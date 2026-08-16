//! TUI 点餐界面（ratatui + crossterm）。
//!
//! 核心点餐链路：门店搜索 → 菜单浏览 → 随心配轮次选配 + 特调 → 购物篮 → 计价 → 下单 → 二维码落盘。

use anyhow::{Context, Result, bail};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use image::Rgb;
use qrcode::QrCode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use serde_json::{Value, json};
use std::time::Duration;

use crate::meal::{self, MealDetail, ModSelection, ModValue, Round};
use crate::mcp::McpClient;

const ORDER_TYPE: i32 = 1;
const BE_TYPE: i32 = 1;

// ---------- 页面 ----------

#[derive(Clone, PartialEq)]
enum Screen {
    StoreSearch,
    StoreList,
    Menu,
    Combo,
    Mods,
    Basket,
    Price,
    OrderResult,
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    City,
    Keyword,
}

struct StoreItem {
    code: String,
    name: String,
    distance: String,
    address: String,
}

#[derive(Clone)]
struct MenuItem {
    code: String,
    name: String,
    price: String,
    category: String,
    with_order: bool,
}

struct BasketItem {
    label: String,
    qty: u32,
    item: Value,
}

struct PriceInfo {
    lines: Vec<String>,
    take_ways: Vec<(String, String)>, // (title, code)
}

#[derive(Clone)]
struct ModGroup {
    item: Vec<ModValue>,
    min: i64,
    max: i64,
    multi: bool,
}

pub struct App {
    client: McpClient,
    screen: Screen,
    should_exit: bool,
    status: String,
    status_kind: StatusKind,

    // 门店搜索
    city: String,
    keyword: String,
    focus: Focus,
    stores: Vec<StoreItem>,
    store_state: ListState,
    store_code: Option<String>,
    store_name: String,

    // 菜单
    menu: Vec<MenuItem>,
    menu_state: ListState,

    // 选餐
    detail: Option<MealDetail>,
    combo_round: usize,
    combo_toggle: Vec<bool>, // 当前轮次各 choice 是否选中
    combo_state: ListState,
    combo_picks: Vec<Vec<usize>>, // 已确认的每轮选中 indices
    combo_mods: Vec<Vec<Option<ModSelection>>>, // 每轮每个选中 choice 的特调

    // 特调
    mods_choice_idx: usize, // 当前处理的选中的 choice 序号
    mods_groups: Vec<ModGroup>, // 当前 choice 的待交互 group
    mods_group_idx: usize,
    mods_toggle: Vec<bool>,
    mods_single: usize,
    mods_state: ListState,
    mods_values: Vec<Value>, // 当前 choice 累积的 modification values

    // 购物篮
    basket: Vec<BasketItem>,
    basket_state: ListState,

    // 计价
    price: Option<PriceInfo>,
    takeway_state: ListState,

    // 下单结果
    order_id: String,
    order_pay_url: String,
    qr_path: String,
}

#[derive(Clone, Copy, PartialEq)]
enum StatusKind {
    Info,
    Error,
}

// ---------- 网络辅助 ----------

async fn fetch_nearby(client: &McpClient, city: &str, keyword: &str) -> Result<Vec<StoreItem>> {
    let mut args = json!({
        "beType": BE_TYPE,
        "searchType": 2,
    });
    if !city.is_empty() {
        args["city"] = Value::String(city.to_string());
    }
    if !keyword.is_empty() {
        args["keyword"] = Value::String(keyword.to_string());
    }
    let result = client.call_tool("query-nearby-stores", args).await?;
    let data = extract_data(&result)?;
    let arr = data.as_array().cloned().unwrap_or_default();
    Ok(arr
        .iter()
        .map(|s| StoreItem {
            code: s.get("storeCode").and_then(|v| v.as_str()).unwrap_or("-").to_string(),
            name: s.get("storeName").and_then(|v| v.as_str()).unwrap_or("-").to_string(),
            distance: s
                .get("distance")
                .and_then(|v| v.as_i64())
                .map(|d| format!("{d}m"))
                .unwrap_or_default(),
            address: s.get("address").and_then(|v| v.as_str()).unwrap_or("-").to_string(),
        })
        .collect())
}

async fn fetch_menu(client: &McpClient, store: &str) -> Result<Vec<MenuItem>> {
    let args = json!({"storeCode": store, "orderType": ORDER_TYPE, "beType": BE_TYPE});
    let result = client.call_tool("query-meals", args).await?;
    let data = extract_data(&result)?;

    let meals_map = data
        .get("meals")
        .and_then(|m| m.as_object())
        .cloned()
        .unwrap_or_default();
    let categories = data
        .get("categories")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    let mut items = Vec::new();
    for cat in categories {
        let cat_name = cat
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("未分类")
            .replace('\n', " ")
            .replace('\r', "");
        if let Some(meals) = cat.get("meals").and_then(|m| m.as_array()) {
            for meal in meals {
                let code = meal.get("code").and_then(|c| c.as_str()).unwrap_or("-");
                let info = meals_map.get(code);
                let name = info
                    .and_then(|i| i.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("-");
                let price = info
                    .and_then(|i| i.get("currentPrice"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("-");
                let with_order = info
                    .and_then(|i| i.get("discountType"))
                    .and_then(|v| v.as_str())
                    .map(|v| v.contains("随单购"))
                    .unwrap_or(false)
                    || info
                        .and_then(|i| i.get("canWithOrder"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                items.push(MenuItem {
                    code: code.to_string(),
                    name: name.to_string(),
                    price: price.to_string(),
                    category: cat_name.clone(),
                    with_order,
                });
            }
        }
    }
    Ok(items)
}

async fn fetch_detail(client: &McpClient, store: &str, code: &str) -> Result<MealDetail> {
    let args = json!({"code": code, "storeCode": store, "orderType": ORDER_TYPE, "beType": BE_TYPE});
    let result = client.call_tool("query-meal-detail", args).await?;
    let data = extract_data(&result)?;
    meal::parse_detail(&data)
}

async fn calc_price(
    client: &McpClient,
    store: &str,
    items: &[Value],
) -> Result<PriceInfo> {
    let args = json!({
        "storeCode": store,
        "orderType": ORDER_TYPE,
        "beType": BE_TYPE,
        "items": items,
    });
    let result = client.call_tool("calculate-price", args).await?;
    let data = extract_data(&result)?;

    let mut lines = Vec::new();
    if let Some(products) = data.get("productList").and_then(|v| v.as_array()) {
        for p in products {
            let name = p.get("productName").and_then(|v| v.as_str()).unwrap_or("-");
            let qty = p.get("quantity").and_then(|v| v.as_i64()).unwrap_or(0);
            let sub = p.get("subtotal").and_then(|v| v.as_i64()).unwrap_or(0);
            lines.push(format!("{name} x{qty}  ¥{:.2}", sub as f64 / 100.0));
        }
    }
    let price = data.get("price").and_then(|v| v.as_i64()).unwrap_or(0);
    let delivery = data.get("deliveryPrice").and_then(|v| v.as_i64()).unwrap_or(0);
    let packing = data.get("packingPrice").and_then(|v| v.as_i64()).unwrap_or(0);
    let discount = data.get("discount").and_then(|v| v.as_i64()).unwrap_or(0);
    if delivery > 0 {
        lines.push(format!("配送费  ¥{:.2}", delivery as f64 / 100.0));
    }
    if packing > 0 {
        lines.push(format!("打包费  ¥{:.2}", packing as f64 / 100.0));
    }
    if discount > 0 {
        lines.push(format!("优惠  -¥{:.2}", discount as f64 / 100.0));
    }
    lines.push(format!("实付合计  ¥{:.2}", price as f64 / 100.0));

    let take_ways = data
        .get("takeWayList")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|tw| {
                    let title = tw.get("title").and_then(|v| v.as_str()).unwrap_or("-");
                    let sub = tw.get("subtitle").and_then(|v| v.as_str()).unwrap_or("");
                    let code = tw.get("code").and_then(|v| v.as_str()).unwrap_or("");
                    (format!("{title} — {sub}"), code.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(PriceInfo { lines, take_ways })
}

fn extract_data(result: &crate::mcp::ToolResult) -> Result<Value> {
    if let Some(ref structured) = result.structured_content {
        return Ok(structured
            .get("data")
            .cloned()
            .unwrap_or_else(|| structured.clone()));
    }
    let text = result
        .content
        .iter()
        .filter_map(|c| c.text.clone())
        .collect::<Vec<_>>()
        .join("");
    if let Some(idx) = text.find("{\"success\":")
        && let Ok(json_val) = serde_json::from_str::<Value>(&text[idx..])
    {
        return Ok(json_val.get("data").cloned().unwrap_or(Value::Null));
    }
    bail!("MCP 返回无法解析")
}

// ---------- 应用逻辑 ----------

impl App {
    pub fn new(client: McpClient) -> Self {
        App {
            client,
            screen: Screen::StoreSearch,
            should_exit: false,
            status: "输入城市和关键词，Enter 搜索门店".to_string(),
            status_kind: StatusKind::Info,
            city: String::new(),
            keyword: String::new(),
            focus: Focus::City,
            stores: Vec::new(),
            store_state: ListState::default(),
            store_code: None,
            store_name: String::new(),
            menu: Vec::new(),
            menu_state: ListState::default(),
            detail: None,
            combo_round: 0,
            combo_toggle: Vec::new(),
            combo_state: ListState::default(),
            combo_picks: Vec::new(),
            combo_mods: Vec::new(),
            mods_choice_idx: 0,
            mods_groups: Vec::new(),
            mods_group_idx: 0,
            mods_toggle: Vec::new(),
            mods_single: 0,
            mods_state: ListState::default(),
            mods_values: Vec::new(),
            basket: Vec::new(),
            basket_state: ListState::default(),
            price: None,
            takeway_state: ListState::default(),
            order_id: String::new(),
            order_pay_url: String::new(),
            qr_path: String::new(),
        }
    }

    pub async fn run(mut self) -> Result<()> {
        let mut terminal = ratatui::init();
        let res = self.run_loop(&mut terminal).await;
        ratatui::restore();
        res
    }

    async fn run_loop(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        while !self.should_exit {
            terminal.draw(|f| self.render(f))?;
            if event::poll(Duration::from_millis(120))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                self.handle_key(key.code).await?;
            }
        }
        Ok(())
    }

    fn set_status(&mut self, msg: impl Into<String>, kind: StatusKind) {
        self.status = msg.into();
        self.status_kind = kind;
    }

    async fn handle_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Char('q') | KeyCode::Esc if self.screen != Screen::StoreSearch => {
                self.go_back();
                return Ok(());
            }
            KeyCode::Char('q') => {
                self.should_exit = true;
                return Ok(());
            }
            _ => {}
        }

        match self.screen {
            Screen::StoreSearch => self.handle_store_search(code).await?,
            Screen::StoreList => self.handle_store_list(code).await?,
            Screen::Menu => self.handle_menu(code).await?,
            Screen::Combo => self.handle_combo(code).await?,
            Screen::Mods => self.handle_mods(code).await?,
            Screen::Basket => self.handle_basket(code).await?,
            Screen::Price => self.handle_price(code).await?,
            Screen::OrderResult => {
                self.should_exit = true;
            }
        }
        Ok(())
    }

    fn go_back(&mut self) {
        self.screen = match self.screen {
            Screen::StoreList => Screen::StoreSearch,
            Screen::Menu => Screen::StoreList,
            Screen::Combo | Screen::Mods => Screen::Menu,
            Screen::Basket => Screen::Menu,
            Screen::Price => Screen::Basket,
            Screen::OrderResult => Screen::Basket,
            Screen::StoreSearch => Screen::StoreSearch,
        };
        self.set_status("已返回", StatusKind::Info);
    }

    // 门店搜索
    async fn handle_store_search(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Tab => {
                self.focus = if self.focus == Focus::City {
                    Focus::Keyword
                } else {
                    Focus::City
                };
            }
            KeyCode::Enter => {
                self.set_status("正在搜索门店...", StatusKind::Info);
                let city = self.city.trim().to_string();
                let keyword = self.keyword.trim().to_string();
                match fetch_nearby(&self.client, &city, &keyword).await {
                    Ok(stores) => {
                        self.stores = stores;
                        self.store_state = ListState::default();
                        if !self.stores.is_empty() {
                            self.store_state.select(Some(0));
                            self.screen = Screen::StoreList;
                            self.set_status("↑↓ 选择门店，Enter 进入菜单，Esc 返回", StatusKind::Info);
                        } else {
                            self.set_status("未找到门店，请换个关键词", StatusKind::Error);
                        }
                    }
                    Err(e) => self.set_status(format!("搜索失败: {e}"), StatusKind::Error),
                }
            }
            _ => self.edit_input(code),
        }
        Ok(())
    }

    fn edit_input(&mut self, code: KeyCode) {
        let s = if self.focus == Focus::City {
            &mut self.city
        } else {
            &mut self.keyword
        };
        match code {
            KeyCode::Char(c) => {
                s.push(c);
            }
            KeyCode::Backspace => {
                s.pop();
            }
            _ => {}
        }
    }

    async fn handle_store_list(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => list_prev(&mut self.store_state, self.stores.len()),
            KeyCode::Down | KeyCode::Char('j') => {
                list_next(&mut self.store_state, self.stores.len())
            }
            KeyCode::Enter => {
                if let Some(i) = self.store_state.selected() {
                    let (code, name) = {
                        let store = &self.stores[i];
                        (store.code.clone(), store.name.clone())
                    };
                    self.store_code = Some(code.clone());
                    self.store_name = name;
                    self.set_status("正在加载菜单...", StatusKind::Info);
                    match fetch_menu(&self.client, &code).await {
                        Ok(menu) => {
                            self.menu = menu;
                            self.menu_state = ListState::default();
                            if !self.menu.is_empty() {
                                self.menu_state.select(Some(0));
                            }
                            self.screen = Screen::Menu;
                            self.set_status(
                                "↑↓ 浏览，Enter 加购，b 购物篮，Esc 返回",
                                StatusKind::Info,
                            );
                        }
                        Err(e) => self.set_status(format!("加载菜单失败: {e}"), StatusKind::Error),
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    // 菜单
    async fn handle_menu(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => list_prev(&mut self.menu_state, self.menu.len()),
            KeyCode::Down | KeyCode::Char('j') => list_next(&mut self.menu_state, self.menu.len()),
            KeyCode::Char('b') => {
                self.open_basket();
            }
            KeyCode::Enter => {
                if let Some(i) = self.menu_state.selected() {
                    let item = self.menu[i].clone();
                    if item.with_order {
                        self.set_status("⚠️ 随单购商品不能单独下单（会按原价结算）", StatusKind::Error);
                        return Ok(());
                    }
                    let store = self.store_code.clone().unwrap_or_default();
                    self.set_status(format!("正在加载 {} 详情...", item.name), StatusKind::Info);
                    match fetch_detail(&self.client, &store, &item.code).await {
                        Ok(detail) => {
                            if detail.rounds.is_empty() {
                                // 无轮次，直接加篮
                                let item_val = json!({"productCode": detail.code, "quantity": 1});
                                self.add_to_basket(detail.name.clone(), 1, item_val);
                                self.screen = Screen::Menu;
                                self.set_status(format!("已加购 {}", detail.name), StatusKind::Info);
                            } else {
                                // 进入选餐
                                self.detail = Some(detail);
                                self.combo_round = 0;
                                self.combo_picks = vec![Vec::new(); self.rounds().len()];
                                self.combo_mods = vec![Vec::new(); self.rounds().len()];
                                self.init_combo_round();
                                self.screen = Screen::Combo;
                            }
                        }
                        Err(e) => self.set_status(format!("加载详情失败: {e}"), StatusKind::Error),
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn rounds(&self) -> &[Round] {
        self.detail.as_ref().map(|d| d.rounds.as_slice()).unwrap_or(&[])
    }

    fn init_combo_round(&mut self) {
        let round = &self.rounds()[self.combo_round];
        self.combo_toggle = round
            .choices
            .iter()
            .map(|c| c.quantity > 0 || c.is_default == 1)
            .collect();
        self.combo_state = ListState::default();
        self.combo_state.select(Some(0));
    }

    fn combo_round_min(&self) -> i64 {
        self.rounds().get(self.combo_round).map(|r| r.min_quantity).unwrap_or(1)
    }
    fn combo_round_max(&self) -> i64 {
        self.rounds().get(self.combo_round).map(|r| r.max_quantity).unwrap_or(1)
    }

    async fn handle_combo(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                list_prev(&mut self.combo_state, self.combo_toggle.len())
            }
            KeyCode::Down | KeyCode::Char('j') => {
                list_next(&mut self.combo_state, self.combo_toggle.len())
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                let i = self.combo_state.selected().unwrap_or(0);
                let min = self.combo_round_min();
                let max = self.combo_round_max();
                if max == 1 && min == 1 {
                    // 单选，直接选中并推进
                    self.combo_toggle = vec![false; self.combo_toggle.len()];
                    self.combo_toggle[i] = true;
                    self.confirm_combo_round().await?;
                } else {
                    // 多选：空格切换，Enter 确认
                    if code == KeyCode::Char(' ') {
                        let selected: i64 = self
                            .combo_toggle
                            .iter()
                            .filter(|b| **b)
                            .count()
                            .try_into()
                            .unwrap_or(0);
                        if self.combo_toggle[i] || selected < max {
                            self.combo_toggle[i] = !self.combo_toggle[i];
                            // 若超过 max，取消最老的一个选中
                            let mut cur: i64 = self
                                .combo_toggle
                                .iter()
                                .filter(|b| **b)
                                .count()
                                .try_into()
                                .unwrap_or(0);
                            while cur > max {
                                if let Some(pos) = self.combo_toggle.iter().position(|b| *b) {
                                    self.combo_toggle[pos] = false;
                                }
                                cur -= 1;
                            }
                        }
                    } else {
                        // Enter 确认
                        let selected: i64 = self
                            .combo_toggle
                            .iter()
                            .filter(|b| **b)
                            .count()
                            .try_into()
                            .unwrap_or(0);
                        if selected < min || selected > max {
                            self.set_status(
                                format!("请选择 {min}~{max} 项"),
                                StatusKind::Error,
                            );
                            return Ok(());
                        }
                        self.confirm_combo_round().await?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn confirm_combo_round(&mut self) -> Result<()> {
        let round_idx = self.combo_round;
        let picks: Vec<usize> = self
            .combo_toggle
            .iter()
            .enumerate()
            .filter(|(_, b)| **b)
            .map(|(i, _)| i)
            .collect();
        self.combo_picks[round_idx] = picks;

        // 收集本轮选中 choice 中需要交互的特调 group
        self.mods_choice_idx = 0;
        self.combo_mods[round_idx] = vec![None; self.combo_picks[round_idx].len()];
        self.process_next_mods_choice(round_idx).await
    }

    async fn process_next_mods_choice(&mut self, round_idx: usize) -> Result<()> {
        if !self.screen_is_combo_ctx() {
            return Ok(());
        }
        // 找下一个有特调的选中 choice
        loop {
            let picks = self.combo_picks[round_idx].clone();
            if self.mods_choice_idx >= picks.len() {
                // 本轮所有 choice 处理完，进入下一轮或加篮
                return self.finish_combo_round_or_add(round_idx).await;
            }
            let choice_idx = picks[self.mods_choice_idx];
            let choice = &self.rounds()[round_idx].choices[choice_idx];
            match &choice.modification {
                Some(modification) if !modification.items.is_empty() => {
                    // 构建 group 列表
                    let mut groups: Vec<ModGroup> = Vec::new();
                    let mut auto: Vec<Value> = Vec::new();
                    for item in &modification.items {
                        if item.values.is_empty() {
                            continue;
                        }
                        let multi = item
                            .values
                            .iter()
                            .all(|v| v.unselected_key.is_some());
                        if item.min_values == 1
                            && item.max_values == 1
                            && item.values.len() == 1
                        {
                            let v = &item.values[0];
                            auto.push(json!({"code": v.code, "key": v.selected_key, "quantity": 1}));
                        } else {
                            groups.push(ModGroup {
                                item: item.values.clone(),
                                min: item.min_values,
                                max: item.max_values,
                                multi,
                            });
                        }
                    }
                    self.mods_values = auto;
                    self.mods_groups = groups;
                    self.mods_group_idx = 0;
                    if self.mods_groups.is_empty() {
                        // 全自动，直接记录
                        self.combo_mods[round_idx][self.mods_choice_idx] =
                            Some(ModSelection { values: self.mods_values.clone() });
                        self.mods_choice_idx += 1;
                        continue;
                    }
                    self.init_mods_group();
                    self.screen = Screen::Mods;
                    return Ok(());
                }
                _ => {
                    self.mods_choice_idx += 1;
                }
            }
        }
    }

    fn screen_is_combo_ctx(&self) -> bool {
        true
    }

    fn init_mods_group(&mut self) {
        let g = &self.mods_groups[self.mods_group_idx];
        if g.multi {
            self.mods_toggle = g
                .item
                .iter()
                .map(|v| v.selected_quantity > 0)
                .collect();
        } else {
            self.mods_single = g
                .item
                .iter()
                .position(|v| v.selected_quantity > 0)
                .unwrap_or(0);
        }
        self.mods_state = ListState::default();
        self.mods_state.select(Some(0));
    }

    async fn handle_mods(&mut self, code: KeyCode) -> Result<()> {
        let len = self.mods_groups.get(self.mods_group_idx).map(|g| g.item.len()).unwrap_or(0);
        let g = self
            .mods_groups
            .get(self.mods_group_idx)
            .cloned()
            .context("特调组缺失")?;
        match code {
            KeyCode::Up | KeyCode::Char('k') => list_prev(&mut self.mods_state, len),
            KeyCode::Down | KeyCode::Char('j') => list_next(&mut self.mods_state, len),
            KeyCode::Char(' ') if g.multi => {
                let i = self.mods_state.selected().unwrap_or(0);
                let selected: i64 = self.mods_toggle.iter().filter(|b| **b).count().try_into().unwrap_or(0);
                if self.mods_toggle[i] || selected < g.max {
                    self.mods_toggle[i] = !self.mods_toggle[i];
                }
            }
            KeyCode::Enter => {
                let i = self.mods_state.selected().unwrap_or(0);
                if g.multi {
                    let selected: i64 = self.mods_toggle.iter().filter(|b| **b).count().try_into().unwrap_or(0);
                    if selected < g.min || selected > g.max {
                        self.set_status(format!("请选择 {}~{} 项", g.min, g.max), StatusKind::Error);
                        return Ok(());
                    }
                    for (idx, v) in g.item.iter().enumerate() {
                        if self.mods_toggle[idx] {
                            self.mods_values
                                .push(json!({"code": v.code, "key": v.selected_key, "quantity": 1}));
                        } else {
                            self.mods_values.push(json!({
                                "code": v.code,
                                "key": v.unselected_key.clone().unwrap_or_default(),
                                "quantity": 0
                            }));
                        }
                    }
                } else {
                    let v = &g.item[i];
                    self.mods_values
                        .push(json!({"code": v.code, "key": v.selected_key, "quantity": 1}));
                }
                // 下一个 group 或结束
                self.mods_group_idx += 1;
                if self.mods_group_idx < self.mods_groups.len() {
                    self.init_mods_group();
                } else {
                    // 当前 choice 特调完成
                    let round_idx = self.combo_round;
                    self.combo_mods[round_idx][self.mods_choice_idx] =
                        Some(ModSelection { values: self.mods_values.clone() });
                    self.mods_choice_idx += 1;
                    self.screen = Screen::Combo;
                    self.process_next_mods_choice(round_idx).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn finish_combo_round_or_add(&mut self, round_idx: usize) -> Result<()> {
        let total = self.rounds().len();
        if round_idx + 1 < total {
            self.combo_round = round_idx + 1;
            self.init_combo_round();
            self.screen = Screen::Combo;
            self.set_status(
                format!(
                    "轮次 {}/{}：{}",
                    self.combo_round + 1,
                    total,
                    self.rounds()[self.combo_round].name
                ),
                StatusKind::Info,
            );
            return Ok(());
        }
        // 生成 item 加入购物篮
        let detail = self.detail.take().context("选餐数据缺失")?;
        let item = meal::build_item_value(&detail, 1, &self.combo_picks, &self.combo_mods, None);
        self.add_to_basket(detail.name.clone(), 1, item);
        self.screen = Screen::Menu;
        self.set_status(format!("已加购 {}", detail.name), StatusKind::Info);
        Ok(())
    }

    fn add_to_basket(&mut self, label: String, qty: u32, item: Value) {
        // 合并相同 label
        if let Some(b) = self.basket.iter_mut().find(|b| b.label == label) {
            b.qty += qty;
            if let Some(n) = b.item.get("quantity").and_then(|v| v.as_u64()) {
                b.item["quantity"] = json!(n + qty as u64);
            }
        } else {
            self.basket.push(BasketItem { label, qty, item });
        }
    }

    fn open_basket(&mut self) {
        self.basket_state = ListState::default();
        if !self.basket.is_empty() {
            self.basket_state.select(Some(0));
        }
        self.screen = Screen::Basket;
        self.set_status("d 删除，p 计价，c 清空，Esc 返回菜单", StatusKind::Info);
    }

    async fn handle_basket(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                list_prev(&mut self.basket_state, self.basket.len())
            }
            KeyCode::Down | KeyCode::Char('j') => {
                list_next(&mut self.basket_state, self.basket.len())
            }
            KeyCode::Char('d') => {
                if let Some(i) = self.basket_state.selected()
                    && i < self.basket.len()
                {
                    self.basket.remove(i);
                    if i >= self.basket.len() {
                        self.basket_state.select(self.basket.len().checked_sub(1));
                    }
                }
            }
            KeyCode::Char('c') => {
                self.basket.clear();
            }
            KeyCode::Char('p') => {
                if self.basket.is_empty() {
                    self.set_status("购物篮为空，先去菜单加购", StatusKind::Error);
                    return Ok(());
                }
                let store = self.store_code.clone().unwrap_or_default();
                let items: Vec<Value> = self.basket.iter().map(|b| b.item.clone()).collect();
                self.set_status("正在计价...", StatusKind::Info);
                match calc_price(&self.client, &store, &items).await {
                    Ok(price) => {
                        self.price = Some(price);
                        self.takeway_state = ListState::default();
                        if let Some(p) = &self.price
                            && !p.take_ways.is_empty()
                        {
                            self.takeway_state.select(Some(0));
                        }
                        self.screen = Screen::Price;
                        self.set_status("↑↓ 选取餐方式，Enter 确认下单", StatusKind::Info);
                    }
                    Err(e) => self.set_status(format!("计价失败: {e}"), StatusKind::Error),
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_price(&mut self, code: KeyCode) -> Result<()> {
        let n = self.price.as_ref().map(|p| p.take_ways.len()).unwrap_or(0);
        match code {
            KeyCode::Up | KeyCode::Char('k') => list_prev(&mut self.takeway_state, n),
            KeyCode::Down | KeyCode::Char('j') => list_next(&mut self.takeway_state, n),
            KeyCode::Enter => {
                let store = self.store_code.clone().unwrap_or_default();
                let items: Vec<Value> = self.basket.iter().map(|b| b.item.clone()).collect();
                let selected = self.takeway_state.selected();
                let take_way = self
                    .price
                    .as_ref()
                    .and_then(|p| selected.and_then(|i| p.take_ways.get(i)))
                    .map(|(_, code)| code.clone())
                    .unwrap_or_default();
                let mut args = json!({
                    "storeCode": store,
                    "items": items,
                    "orderType": ORDER_TYPE,
                    "beType": BE_TYPE,
                });
                if !take_way.is_empty() {
                    args["takeWayCode"] = Value::String(take_way);
                }
                self.set_status("正在创建订单...", StatusKind::Info);
                match self.client.call_tool("create-order", args).await {
                    Ok(result) => {
                        let data = extract_data(&result)?;
                        let order_id = data
                            .get("orderId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string();
                        let pay_url = data
                            .get("payH5Url")
                            .and_then(|v| v.as_str())
                            .unwrap_or("-")
                            .to_string();
                        self.order_id = order_id.clone();
                        self.order_pay_url = pay_url.clone();
                        self.qr_path = save_qr(&pay_url, &order_id).unwrap_or_else(|e| format!("(二维码保存失败: {e})"));
                        self.screen = Screen::OrderResult;
                        self.set_status("下单成功！q 退出", StatusKind::Info);
                    }
                    Err(e) => self.set_status(format!("下单失败: {e}"), StatusKind::Error),
                }
            }
            _ => {}
        }
        Ok(())
    }

    }

// ---------- 二维码落盘 ----------

fn save_qr(url: &str, order_id: &str) -> Result<String, String> {
    let code = QrCode::new(url.as_bytes()).map_err(|e| e.to_string())?;
    let img = code
        .render::<Rgb<u8>>()
        .min_dimensions(300, 300)
        .quiet_zone(true)
        .build();
    let dir = "/tmp/mcd-qrcode";
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = format!("{dir}/{order_id}.png");
    img.save(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

// ---------- 渲染 ----------

impl App {
    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area);

        self.render_header(frame, chunks[0]);
        match self.screen {
            Screen::StoreSearch => self.render_store_search(frame, chunks[1]),
            Screen::StoreList => self.render_store_list(frame, chunks[1]),
            Screen::Menu => self.render_menu(frame, chunks[1]),
            Screen::Combo => self.render_combo(frame, chunks[1]),
            Screen::Mods => self.render_mods(frame, chunks[1]),
            Screen::Basket => self.render_basket(frame, chunks[1]),
            Screen::Price => self.render_price(frame, chunks[1]),
            Screen::OrderResult => self.render_order_result(frame, chunks[1]),
        }
        self.render_status(frame, chunks[2]);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let store = if self.store_name.is_empty() {
            "（未选门店）".to_string()
        } else {
            self.store_name.clone()
        };
        let title = Paragraph::new(vec![
            Line::from(Span::styled(
                "  🍟 麦当劳 TUI 点餐",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("  门店: {store}"),
                Style::default().fg(Color::Cyan),
            )),
        ])
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(title, area);
    }

    fn render_status(&self, frame: &mut Frame, area: Rect) {
        let color = match self.status_kind {
            StatusKind::Info => Color::Gray,
            StatusKind::Error => Color::Red,
        };
        let p = Paragraph::new(Span::styled(self.status.clone(), Style::default().fg(color)));
        frame.render_widget(p, area);
    }

    fn render_store_search(&self, frame: &mut Frame, area: Rect) {
        let city_line = input_line("城市: ", &self.city, self.focus == Focus::City);
        let keyword_line = input_line("关键词: ", &self.keyword, self.focus == Focus::Keyword);
        let p = Paragraph::new(vec![
            Line::from(city_line),
            Line::from(keyword_line),
            Line::from(""),
            Line::from(Span::styled(
                "Tab 切换输入框 · Enter 搜索 · q 退出",
                Style::default().fg(Color::Gray),
            )),
        ])
        .block(Block::default().title(" 查找门店 ").borders(Borders::ALL));
        frame.render_widget(p, area);
    }

    fn render_store_list(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .stores
            .iter()
            .map(|s| {
                ListItem::new(vec![
                    Line::from(Span::styled(
                        format!(" {} ", s.name),
                        Style::default().fg(Color::White),
                    )),
                    Line::from(Span::styled(
                        format!("   {} · {}\n", s.distance, s.address),
                        Style::default().fg(Color::Gray),
                    )),
                ])
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().title(" 选择门店 ").borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        frame.render_stateful_widget(list, area, &mut self.store_state);
    }

    fn render_menu(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .menu
            .iter()
            .map(|m| {
                let tag = if m.with_order { " [随单购]" } else { "" };
                ListItem::new(vec![Line::from(vec![
                    Span::styled(
                        format!(" {} ", m.name),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("¥{}", m.price),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(tag, Style::default().fg(Color::Magenta)),
                    Span::styled(
                        format!("   [{}]", m.category),
                        Style::default().fg(Color::Gray),
                    ),
                ])])
            })
            .collect();
        let list = List::new(items)
            .block(Block::default().title(format!(
                " 菜单（{} 件商品） ",
                self.menu.len()
            )).borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        frame.render_stateful_widget(list, area, &mut self.menu_state);
    }

    fn render_combo(&mut self, frame: &mut Frame, area: Rect) {
        let detail = match &self.detail {
            Some(d) => d,
            None => {
                frame.render_widget(Paragraph::new("加载中..."), area);
                return;
            }
        };
        let rounds = &detail.rounds;
        let round = &rounds[self.combo_round];
        let items: Vec<ListItem> = round
            .choices
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mark = if self.combo_toggle[i] { "■ " } else { "□ " };
                let price = c.diff_price.as_deref().unwrap_or("");
                let modify = if c.support_modify { " [可特调]" } else { "" };
                ListItem::new(Line::from(vec![
                    Span::styled(mark, Style::default().fg(Color::Green)),
                    Span::styled(c.name.clone(), Style::default().fg(Color::White)),
                    Span::styled(format!(" {price}"), Style::default().fg(Color::Yellow)),
                    Span::styled(modify, Style::default().fg(Color::Cyan)),
                ]))
            })
            .collect();
        let max = round.max_quantity;
        let min = round.min_quantity;
        let title = format!(
            " 轮次{}/{}【{}】 选{}~{}项 ",
            self.combo_round + 1,
            rounds.len(),
            round.name,
            min,
            max
        );
        let hint = if max == 1 && min == 1 {
            "Enter 选中"
        } else {
            "空格 切换 · Enter 确认"
        };
        let p = List::new(items)
            .block(Block::default().title(title).borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        frame.render_stateful_widget(p, area, &mut self.combo_state);
        frame.render_widget(
            Paragraph::new(Span::styled(hint, Style::default().fg(Color::Gray)))
                .block(Block::default()),
            Rect::new(area.x, area.y + area.height.saturating_sub(1), area.width, 1),
        );
    }

    fn render_mods(&mut self, frame: &mut Frame, area: Rect) {
        let Some(g) = self.mods_groups.get(self.mods_group_idx).cloned() else {
            frame.render_widget(Paragraph::new("特调完成"), area);
            return;
        };
        let items: Vec<ListItem> = g
            .item
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let mark = if g.multi {
                    if self.mods_toggle[i] { "■ " } else { "□ " }
                } else if i == self.mods_single {
                    "● "
                } else {
                    "○ "
                };
                ListItem::new(Line::from(vec![
                    Span::styled(mark, Style::default().fg(Color::Green)),
                    Span::styled(v.name.clone(), Style::default().fg(Color::White)),
                ]))
            })
            .collect();
        let title = format!(" 特调（选{}~{}项） ", g.min, g.max);
        let hint = if g.multi { "空格 切换 · Enter 确认" } else { "↑↓ 选择 · Enter 确认" };
        let list = List::new(items)
            .block(Block::default().title(title).borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        frame.render_stateful_widget(list, area, &mut self.mods_state);
        frame.render_widget(
            Paragraph::new(Span::styled(hint, Style::default().fg(Color::Gray))),
            Rect::new(area.x, area.y + area.height.saturating_sub(1), area.width, 1),
        );
    }

    fn render_basket(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .basket
            .iter()
            .map(|b| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {} ", b.label), Style::default().fg(Color::White)),
                    Span::styled(format!("x{}", b.qty), Style::default().fg(Color::Yellow)),
                ]))
            })
            .collect();
        let body = if self.basket.is_empty() {
            Paragraph::new("购物篮为空，回菜单加购吧")
        } else {
            Paragraph::new("")
        };
        let list = List::new(items)
            .block(Block::default().title(format!(
                " 购物篮（{} 件） ",
                self.basket.iter().map(|b| b.qty).sum::<u32>()
            )).borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        if self.basket.is_empty() {
            frame.render_widget(body.block(Block::default().borders(Borders::ALL)), area);
        } else {
            frame.render_stateful_widget(list, area, &mut self.basket_state);
        }
    }

    fn render_price(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(8), Constraint::Min(1)])
            .split(area);
        let lines = self.price.as_ref().map(|p| p.lines.clone()).unwrap_or_default();
        let text = Text::from(lines.join("\n")).patch_style(Style::default().fg(Color::White));
        let summary = Paragraph::new(text).block(Block::default().title(" 价格 ").borders(Borders::ALL));
        frame.render_widget(summary, chunks[0]);

        let take_ways = self
            .price
            .as_ref()
            .map(|p| p.take_ways.clone())
            .unwrap_or_default();
        let items: Vec<ListItem> = take_ways
            .iter()
            .map(|(t, _)| ListItem::new(Span::styled(t.clone(), Style::default().fg(Color::White))))
            .collect();
        let list = List::new(items)
            .block(Block::default().title(" 取餐方式 ").borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        frame.render_stateful_widget(list, chunks[1], &mut self.takeway_state);
    }

    fn render_order_result(&self, frame: &mut Frame, area: Rect) {
        let lines = vec![
            Line::from(Span::styled(
                "🎉 下单成功！",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::styled("订单号: ", Style::default().fg(Color::Gray)),
                Span::styled(self.order_id.clone(), Style::default().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::styled("支付链接: ", Style::default().fg(Color::Gray)),
                Span::styled(self.order_pay_url.clone(), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("二维码: ", Style::default().fg(Color::Gray)),
                Span::styled(self.qr_path.clone(), Style::default().fg(Color::Green)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "用麦当劳 APP / 微信扫上面的二维码付款，q 退出",
                Style::default().fg(Color::Gray),
            )),
        ];
        let p = Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(Block::default().title(" 订单 ").borders(Borders::ALL));
        frame.render_widget(p, area);
    }
}

fn list_prev(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let i = state.selected().unwrap_or(0);
    let next = if i == 0 { len - 1 } else { i - 1 };
    state.select(Some(next));
}

fn list_next(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let i = state.selected().unwrap_or(0);
    let next = if i + 1 >= len { 0 } else { i + 1 };
    state.select(Some(next));
}

pub async fn run(client: McpClient) -> Result<()> {
    App::new(client).run().await
}

fn input_line(label: &str, value: &str, active: bool) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(
        label.to_string(),
        Style::default().fg(Color::Gray),
    )];
    let style = if active {
        Style::default().fg(Color::White).bg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };
    spans.push(Span::styled(
        format!("{}▎", value),
        style,
    ));
    spans
}