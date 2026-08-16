//! TUI 点餐界面（ratatui + crossterm）。
//!
//! 核心点餐链路：门店搜索 → 菜单浏览 → 随心配轮次选配 + 特调 → 购物篮 → 计价 → 下单 → 二维码落盘。

use anyhow::{Context, Result, bail};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
        MouseEventKind,
    },
    execute,
};
use image::Rgb;
use qrcode::QrCode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use serde_json::{Value, json};
use std::time::Duration;
use unicode_width::UnicodeWidthChar;

use crate::meal::{self, Choice, MealDetail, ModSelection, ModValue, Modification, Round};
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
    Price,
    OrderResult,
}

#[derive(Clone, Copy, PartialEq)]
enum Focus {
    City,
    Keyword,
}

#[derive(Clone, Copy, PartialEq)]
enum MenuFocus {
    Search,
    Category,
    Items,
    Basket,
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
    detail: String, // 套餐子项 + 特调展示（如 "双层吉士汉堡 + 可乐麦炫酷（去冰）"）
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
    menu_categories: Vec<String>,
    menu_cat_state: ListState,
    menu_search: String,
    menu_focus: MenuFocus,

    // 选餐（所有轮次一页）
    detail: Option<MealDetail>,
    combo_toggles: Vec<Vec<bool>>, // 每轮各 choice 是否选中
    combo_state: ListState,
    combo_picks: Vec<Vec<usize>>, // 已确认的每轮选中 indices
    combo_mods: Vec<Vec<Option<ModSelection>>>, // 每轮每个选中 choice 的特调
    combo_mods_map: Vec<Vec<Option<ModSelection>>>, // 每轮每 choice 的即时特调结果
    mods_target_ri: usize, // 当前即时特调目标轮次
    mods_target_ci: usize, // 当前即时特调目标 choice

    // 特调
    mods_groups: Vec<ModGroup>, // 当前 choice 的待交互 group
    mods_group_idx: usize,
    mods_toggle: Vec<bool>,
    mods_single: usize,
    mods_state: ListState,
    mods_values: Vec<Value>, // 当前 choice 累积的 modification values
    product_mods_active: bool, // true=正在处理单品特调（非套餐 choice 特调）

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

    // 最近一次渲染区域（鼠标定位用）
    last_area: Rect,
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
            menu_categories: Vec::new(),
            menu_cat_state: ListState::default(),
            menu_search: String::new(),
            menu_focus: MenuFocus::Items,
            detail: None,
            combo_toggles: Vec::new(),
            combo_state: ListState::default(),
            combo_picks: Vec::new(),
            combo_mods: Vec::new(),
            combo_mods_map: Vec::new(),
            mods_target_ri: 0,
            mods_target_ci: 0,
            mods_groups: Vec::new(),
            mods_group_idx: 0,
            mods_toggle: Vec::new(),
            mods_single: 0,
            mods_state: ListState::default(),
            mods_values: Vec::new(),
            product_mods_active: false,
            basket: Vec::new(),
            basket_state: ListState::default(),
            price: None,
            takeway_state: ListState::default(),
            order_id: String::new(),
            order_pay_url: String::new(),
            qr_path: String::new(),
            last_area: Rect::default(),
        }
    }

    pub async fn run(mut self) -> Result<()> {
        let mut terminal = ratatui::init();
        execute!(std::io::stdout(), EnableMouseCapture).context("启用鼠标捕获失败")?;
        let res = self.run_loop(&mut terminal).await;
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
        ratatui::restore();
        res
    }

    async fn run_loop(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        while !self.should_exit {
            terminal.draw(|f| self.render(f))?;
            if event::poll(Duration::from_millis(120))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        self.handle_key(key.code).await?;
                    }
                    Event::Mouse(mouse) => {
                        self.handle_mouse(mouse).await?;
                    }
                    Event::Resize(..) => {}
                    _ => {}
                }
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
            KeyCode::Char('q') if self.screen != Screen::StoreSearch => {
                self.go_back();
                return Ok(());
            }
            KeyCode::Esc
                if self.screen != Screen::StoreSearch
                    && !(self.screen == Screen::Menu && self.menu_focus == MenuFocus::Search) =>
            {
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
            Screen::Combo => Screen::Menu,
            Screen::Mods => {
                if self.product_mods_active {
                    self.product_mods_active = false;
                    Screen::Menu
                } else {
                    Screen::Combo
                }
            }
            Screen::Price => Screen::Menu,
            Screen::OrderResult => Screen::Menu,
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
                            let mut cats: Vec<String> = Vec::new();
                            for m in &menu {
                                if !cats.contains(&m.category) {
                                    cats.push(m.category.clone());
                                }
                            }
                            self.menu_categories = cats;
                            self.menu = menu;
                            self.menu_state = ListState::default();
                            self.menu_cat_state = ListState::default();
                            self.menu_cat_state.select(Some(0));
                            self.menu_search.clear();
                            self.menu_focus = MenuFocus::Items;
                            if !self.menu.is_empty() {
                                self.menu_state.select(Some(0));
                            }
                            self.screen = Screen::Menu;
                            self.set_status(
                                "Tab 切换 搜索/分类/餐品，Enter 加购，b 购物篮",
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
    fn visible_menu(&self) -> Vec<usize> {
        let cat = self.menu_cat_state.selected().and_then(|i| {
            if i == 0 {
                None
            } else {
                self.menu_categories.get(i - 1)
            }
        });
        self.menu
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                let ok_cat = cat.map(|c| m.category == *c).unwrap_or(true);
                let ok_search = self.menu_search.is_empty() || m.name.contains(&self.menu_search);
                ok_cat && ok_search
            })
            .map(|(i, _)| i)
            .collect()
    }

    async fn handle_menu(&mut self, code: KeyCode) -> Result<()> {
        if code == KeyCode::Char('/') && self.menu_focus != MenuFocus::Search {
            // 按 / 快速进入搜索框
            self.menu_focus = MenuFocus::Search;
            self.menu_search.clear();
            return Ok(());
        }
        if code == KeyCode::Tab {
            self.menu_focus = match self.menu_focus {
                MenuFocus::Search => MenuFocus::Category,
                MenuFocus::Category => MenuFocus::Items,
                MenuFocus::Items => MenuFocus::Basket,
                MenuFocus::Basket => MenuFocus::Search,
            };
            return Ok(());
        }
        match code {
            KeyCode::Right if self.menu_focus != MenuFocus::Search => {
                self.menu_focus = match self.menu_focus {
                    MenuFocus::Category => MenuFocus::Items,
                    MenuFocus::Items => MenuFocus::Basket,
                    MenuFocus::Basket => MenuFocus::Category,
                    MenuFocus::Search => MenuFocus::Search,
                };
                return Ok(());
            }
            KeyCode::Left if self.menu_focus != MenuFocus::Search => {
                self.menu_focus = match self.menu_focus {
                    MenuFocus::Category => MenuFocus::Basket,
                    MenuFocus::Items => MenuFocus::Category,
                    MenuFocus::Basket => MenuFocus::Items,
                    MenuFocus::Search => MenuFocus::Search,
                };
                return Ok(());
            }
            _ => {}
        }
        match self.menu_focus {
            MenuFocus::Search => match code {
                KeyCode::Enter | KeyCode::Esc => self.menu_focus = MenuFocus::Items,
                KeyCode::Backspace => {
                    self.menu_search.pop();
                    self.menu_state.select(Some(0));
                }
                KeyCode::Char(c) => {
                    self.menu_search.push(c);
                    self.menu_state.select(Some(0));
                }
                _ => {}
            },
            MenuFocus::Category => match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    list_prev(&mut self.menu_cat_state, self.menu_categories.len() + 1);
                    self.menu_state.select(Some(0));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    list_next(&mut self.menu_cat_state, self.menu_categories.len() + 1);
                    self.menu_state.select(Some(0));
                }
                KeyCode::Enter | KeyCode::Right => self.menu_focus = MenuFocus::Items,
                KeyCode::Char('b') => self.menu_focus = MenuFocus::Basket,
                _ => {}
            },
            MenuFocus::Items => {
                let visible = self.visible_menu();
                match code {
                    KeyCode::Up | KeyCode::Char('k') => list_prev(&mut self.menu_state, visible.len()),
                    KeyCode::Down | KeyCode::Char('j') => list_next(&mut self.menu_state, visible.len()),
                    KeyCode::Char('b') => self.menu_focus = MenuFocus::Basket,
                    KeyCode::Enter => {
                        let Some(sel) = self.menu_state.selected() else { return Ok(()) };
                        let Some(&orig_idx) = visible.get(sel) else { return Ok(()) };
                        let item = self.menu[orig_idx].clone();
                        if item.with_order {
                            self.set_status("⚠️ 随单购商品不能单独下单（会按原价结算）", StatusKind::Error);
                            return Ok(());
                        }
                        let store = self.store_code.clone().unwrap_or_default();
                        self.set_status(format!("正在加载 {} 详情...", item.name), StatusKind::Info);
                        match fetch_detail(&self.client, &store, &item.code).await {
                            Ok(detail) => {
                                if detail.rounds.is_empty() {
                                    // 单品：有特调则进特调流程（如饮料去冰）
                                    if let Some(modification) = detail.modification.as_ref()
                                        && !modification.items.is_empty()
                                    {
                                        let (groups, auto) = build_mod_groups(modification);
                                        self.detail = Some(detail);
                                        self.product_mods_active = true;
                                        self.mods_values = auto;
                                        self.mods_groups = groups;
                                        self.mods_group_idx = 0;
                                        if self.mods_groups.is_empty() {
                                            // 全自动特调，直接加篮
                                            let d = self.detail.take().context("选餐数据缺失")?;
                                            let sel =
                                                ModSelection { values: self.mods_values.clone() };
                                            let names = d
                                                .modification
                                                .as_ref()
                                                .map(|m| mod_names_from_sel(m, &sel))
                                                .unwrap_or_default();
                                            let item =
                                                meal::build_item_value(&d, 1, &[], &[], Some(sel));
                                            self.product_mods_active = false;
                                            self.add_to_basket(d.name.clone(), 1, item, names.join(" / "));
                                            self.screen = Screen::Menu;
                                            self.set_status(format!("已加购 {}", d.name), StatusKind::Info);
                                        } else {
                                            self.init_mods_group();
                                            self.screen = Screen::Mods;
                                        }
                                    } else {
                                        let item_val =
                                            json!({"productCode": detail.code, "quantity": 1});
                                        self.add_to_basket(detail.name.clone(), 1, item_val, String::new());
                                        self.screen = Screen::Menu;
                                        self.set_status(format!("已加购 {}", detail.name), StatusKind::Info);
                                    }
                                } else {
                                    let toggles: Vec<Vec<bool>> = detail
                                        .rounds
                                        .iter()
                                        .map(|round| {
                                            let mut t: Vec<bool> = round
                                                .choices
                                                .iter()
                                                .map(|c| c.quantity > 0 || c.is_default == 1)
                                                .collect();
                                            if round.min_quantity == 1
                                                && round.max_quantity == 1
                                                && let Some(first) = t.iter().position(|b| *b)
                                            {
                                                t = vec![false; t.len()];
                                                t[first] = true;
                                            }
                                            t
                                        })
                                        .collect();
                                    let n = detail.rounds.len();
                                    let mods_map: Vec<Vec<Option<ModSelection>>> = detail
                                        .rounds
                                        .iter()
                                        .map(|r| vec![None; r.choices.len()])
                                        .collect();
                                    self.detail = Some(detail);
                                    self.combo_toggles = toggles;
                                    self.combo_picks = vec![Vec::new(); n];
                                    self.combo_mods = vec![Vec::new(); n];
                                    self.combo_mods_map = mods_map;
                                    self.combo_state = ListState::default();
                                    self.combo_state.select(Some(0));
                                    self.screen = Screen::Combo;
                                }
                            }
                            Err(e) => self.set_status(format!("加载详情失败: {e}"), StatusKind::Error),
                        }
                    }
                    _ => {}
                }
            }
            MenuFocus::Basket => match code {
                KeyCode::Up | KeyCode::Char('k') => list_prev(&mut self.basket_state, self.basket.len()),
                KeyCode::Down | KeyCode::Char('j') => list_next(&mut self.basket_state, self.basket.len()),
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
                KeyCode::Char('c') => self.basket.clear(),
                KeyCode::Char('p') => self.price_basket().await?,
                KeyCode::Char('l') => self.menu_focus = MenuFocus::Items,
                _ => {}
            },
        }
        Ok(())
    }

    async fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) -> Result<()> {
        // crossterm 鼠标坐标是 1-based，ratatui Rect 是 0-based，减 1 对齐
        let col = mouse.column.saturating_sub(1);
        let row = mouse.row.saturating_sub(1);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => match self.screen {
                Screen::StoreSearch => {
                    let content = content_area(self.last_area);
                    if row == content.y + 1 {
                        self.focus = Focus::City;
                    } else if row == content.y + 2 {
                        self.focus = Focus::Keyword;
                    }
                }
                Screen::StoreList => {
                    let popup = centered_rect(72, 82, content_area(self.last_area));
                    let inner = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(1),
                            Constraint::Min(1),
                            Constraint::Length(1),
                        ])
                        .split(popup);
                    if let Some(i) =
                        list_index_at(inner[1], row, self.store_state.offset(), self.stores.len())
                    {
                        self.store_state.select(Some(i));
                    }
                }
                Screen::Menu => self.mouse_menu(col, row),
                Screen::Combo => {
                    let popup = centered_rect(66, 80, content_area(self.last_area));
                    let inner = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(1),
                            Constraint::Min(1),
                            Constraint::Length(1),
                        ])
                        .split(popup);
                    if let Some(i) = list_index_at(
                        inner[1],
                        row,
                        self.combo_state.offset(),
                        self.combo_flat().len(),
                    ) {
                        self.combo_state.select(Some(i));
                    }
                }
                Screen::Mods => {
                    let popup = centered_rect(48, 48, content_area(self.last_area));
                    let inner = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(1),
                            Constraint::Min(1),
                            Constraint::Length(1),
                        ])
                        .split(popup);
                    let len = self
                        .mods_groups
                        .get(self.mods_group_idx)
                        .map(|g| g.item.len())
                        .unwrap_or(0);
                    if let Some(i) = list_index_at(inner[1], row, self.mods_state.offset(), len) {
                        self.mods_state.select(Some(i));
                    }
                }
                Screen::Price => {
                    let area = content_area(self.last_area);
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(8), Constraint::Min(1)])
                        .split(area);
                    let n = self.price.as_ref().map(|p| p.take_ways.len()).unwrap_or(0);
                    if let Some(i) = list_index_at(chunks[1], row, self.takeway_state.offset(), n)
                    {
                        self.takeway_state.select(Some(i));
                    }
                }
                Screen::OrderResult => {}
            },
            MouseEventKind::ScrollUp => self.mouse_scroll(true),
            MouseEventKind::ScrollDown => self.mouse_scroll(false),
            _ => {}
        }
        Ok(())
    }

    fn mouse_menu(&mut self, col: u16, row: u16) {
        let area = self.last_area;
        if row == area.y {
            self.menu_focus = MenuFocus::Search;
            return;
        }
        let panes = menu_panes(area);
        if col < panes[0].right() {
            self.menu_focus = MenuFocus::Category;
            if let Some(i) = list_index_at(
                panes[0],
                row,
                self.menu_cat_state.offset(),
                self.menu_categories.len() + 1,
            ) {
                self.menu_cat_state.select(Some(i));
                self.menu_state.select(Some(0));
            }
        } else if col < panes[1].right() {
            self.menu_focus = MenuFocus::Items;
            let visible = self.visible_menu();
            if let Some(i) = list_index_at(panes[1], row, self.menu_state.offset(), visible.len())
            {
                self.menu_state.select(Some(i));
            }
        } else {
            self.menu_focus = MenuFocus::Basket;
            if let Some(i) = list_index_at(panes[2], row, self.basket_state.offset(), self.basket.len())
            {
                self.basket_state.select(Some(i));
            }
        }
    }

    fn mouse_scroll(&mut self, up: bool) {
        match self.screen {
            Screen::Menu => match self.menu_focus {
                MenuFocus::Category => {
                    if up {
                        list_prev(&mut self.menu_cat_state, self.menu_categories.len() + 1);
                    } else {
                        list_next(&mut self.menu_cat_state, self.menu_categories.len() + 1);
                    }
                }
                MenuFocus::Items => {
                    let visible = self.visible_menu();
                    if up {
                        list_prev(&mut self.menu_state, visible.len());
                    } else {
                        list_next(&mut self.menu_state, visible.len());
                    }
                }
                MenuFocus::Basket => {
                    if up {
                        list_prev(&mut self.basket_state, self.basket.len());
                    } else {
                        list_next(&mut self.basket_state, self.basket.len());
                    }
                }
                MenuFocus::Search => {}
            },
            Screen::StoreList => {
                if up {
                    list_prev(&mut self.store_state, self.stores.len());
                } else {
                    list_next(&mut self.store_state, self.stores.len());
                }
            }
            _ => {}
        }
    }

    fn rounds(&self) -> &[Round] {
        self.detail.as_ref().map(|d| d.rounds.as_slice()).unwrap_or(&[])
    }

    fn combo_flat(&self) -> Vec<(usize, Option<usize>)> {
        let mut flat = Vec::new();
        for (ri, round) in self.rounds().iter().enumerate() {
            flat.push((ri, None));
            for ci in 0..round.choices.len() {
                flat.push((ri, Some(ci)));
            }
        }
        flat
    }

    async fn handle_combo(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.combo_move(true),
            KeyCode::Down | KeyCode::Char('j') => self.combo_move(false),
            KeyCode::Char(' ') => {
                let i = self.combo_state.selected().unwrap_or(0);
                if let Some((ri, Some(ci))) = self.combo_flat().get(i).copied() {
                    self.combo_toggle_choice(ri, ci);
                    self.maybe_enter_mods(ri, ci).await?;
                }
            }
            KeyCode::Enter => {
                let i = self.combo_state.selected().unwrap_or(0);
                if let Some((ri, Some(ci))) = self.combo_flat().get(i).copied() {
                    let round = &self.rounds()[ri];
                    if round.min_quantity == 1 && round.max_quantity == 1 {
                        self.combo_toggle_choice(ri, ci);
                        self.maybe_enter_mods(ri, ci).await?;
                    }
                }
            }
            KeyCode::Char('o') => {
                self.combo_done().await?;
            }
            _ => {}
        }
        Ok(())
    }

    fn combo_move(&mut self, up: bool) {
        let flat = self.combo_flat();
        if flat.is_empty() {
            return;
        }
        let mut i = self.combo_state.selected().unwrap_or(0);
        for _ in 0..flat.len() {
            if up {
                i = if i == 0 { flat.len() - 1 } else { i - 1 };
            } else {
                i = (i + 1) % flat.len();
            }
            if flat[i].1.is_some() {
                break;
            }
        }
        self.combo_state.select(Some(i));
    }

    fn combo_toggle_choice(&mut self, ri: usize, ci: usize) {
        let round = &self.rounds()[ri];
        let min = round.min_quantity;
        let max = round.max_quantity;
        if min == 1 && max == 1 {
            for t in self.combo_toggles[ri].iter_mut() {
                *t = false;
            }
            self.combo_toggles[ri][ci] = true;
        } else {
            let selected = self.combo_toggles[ri].iter().filter(|b| **b).count() as i64;
            if self.combo_toggles[ri][ci] || selected < max {
                self.combo_toggles[ri][ci] = !self.combo_toggles[ri][ci];
            }
        }
    }

    async fn maybe_enter_mods(&mut self, ri: usize, ci: usize) -> Result<()> {
        let choice = &self.rounds()[ri].choices[ci];
        if let Some(modification) = choice.modification.as_ref()
            && !modification.items.is_empty()
        {
            let (groups, auto) = build_mod_groups(modification);
            self.mods_values = auto;
            self.mods_groups = groups;
            self.mods_group_idx = 0;
            if self.mods_groups.is_empty() {
                // 全自动，直接记录
                self.combo_mods_map[ri][ci] =
                    Some(ModSelection { values: self.mods_values.clone() });
            } else {
                self.mods_target_ri = ri;
                self.mods_target_ci = ci;
                self.init_mods_group();
                self.screen = Screen::Mods;
            }
        }
        Ok(())
    }

    async fn combo_done(&mut self) -> Result<()> {
        for (ri, round) in self.rounds().iter().enumerate() {
            let n = self.combo_toggles[ri].iter().filter(|b| **b).count() as i64;
            if n < round.min_quantity || n > round.max_quantity {
                self.set_status(
                    format!(
                        "轮次{}【{}】请选择 {}~{} 项",
                        ri + 1,
                        round.name,
                        round.min_quantity,
                        round.max_quantity
                    ),
                    StatusKind::Error,
                );
                return Ok(());
            }
        }
        // 构建 picks
        self.combo_picks = self
            .combo_toggles
            .iter()
            .map(|t| {
                t.iter()
                    .enumerate()
                    .filter(|(_, b)| **b)
                    .map(|(i, _)| i)
                    .collect()
            })
            .collect();
        self.combo_mods = self
            .combo_picks
            .iter()
            .enumerate()
            .map(|(ri, picks)| {
                picks
                    .iter()
                    .map(|&ci| self.combo_mods_map[ri][ci].clone())
                    .collect()
            })
            .collect();
        self.finish_add_combo().await
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
                } else if self.product_mods_active {
                    // 单品特调完成，加篮
                    let detail = self.detail.take().context("选餐数据缺失")?;
                    let sel = ModSelection { values: self.mods_values.clone() };
                    let names = detail
                        .modification
                        .as_ref()
                        .map(|m| mod_names_from_sel(m, &sel))
                        .unwrap_or_default();
                    let item = meal::build_item_value(&detail, 1, &[], &[], Some(sel));
                    self.product_mods_active = false;
                    self.add_to_basket(detail.name.clone(), 1, item, names.join(" / "));
                    self.screen = Screen::Menu;
                    self.set_status(format!("已加购 {}", detail.name), StatusKind::Info);
                } else {
                    // 即时特调（套餐 choice）完成，存结果回到选餐页
                    let ri = self.mods_target_ri;
                    let ci = self.mods_target_ci;
                    self.combo_mods_map[ri][ci] =
                        Some(ModSelection { values: self.mods_values.clone() });
                    self.screen = Screen::Combo;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn finish_add_combo(&mut self) -> Result<()> {
        // 所有轮次选配+特调完成，生成 item 加入购物篮
        let detail = self.detail.take().context("选餐数据缺失")?;
        let item = meal::build_item_value(&detail, 1, &self.combo_picks, &self.combo_mods, None);
        let sub = Self::combo_sub_lines(&detail, &self.combo_picks, &self.combo_mods);
        self.add_to_basket(detail.name.clone(), 1, item, sub.join(" + "));
        self.screen = Screen::Menu;
        self.set_status(format!("已加购 {}", detail.name), StatusKind::Info);
        Ok(())
    }

    fn add_to_basket(&mut self, label: String, qty: u32, item: Value, detail: String) {
        // 合并相同 label
        if let Some(b) = self.basket.iter_mut().find(|b| b.label == label) {
            b.qty += qty;
            if let Some(n) = b.item.get("quantity").and_then(|v| v.as_u64()) {
                b.item["quantity"] = json!(n + qty as u64);
            }
        } else {
            self.basket.push(BasketItem { label, qty, item, detail });
        }
    }

    fn combo_sub_lines(
        detail: &MealDetail,
        picks: &[Vec<usize>],
        mods: &[Vec<Option<ModSelection>>],
    ) -> Vec<String> {
        let mut lines = Vec::new();
        for (ri, round) in detail.rounds.iter().enumerate() {
            let Some(round_picks) = picks.get(ri) else { continue };
            for (pi, &ci) in round_picks.iter().enumerate() {
                if let Some(choice) = round.choices.get(ci) {
                    let mut line = choice.name.clone();
                    if let Some(Some(sel)) = mods.get(ri).and_then(|m| m.get(pi)) {
                        let names = mod_names(choice, sel);
                        if !names.is_empty() {
                            line.push_str(&format!("（{}）", names.join(" / ")));
                        }
                    }
                    lines.push(line);
                }
            }
        }
        lines
    }

    async fn price_basket(&mut self) -> Result<()> {
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
        self.last_area = area;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(2),
            ])
            .split(area);

        self.render_header(frame, chunks[0]);
        match self.screen {
            Screen::StoreSearch => self.render_store_search(frame, chunks[1]),
            Screen::StoreList => self.render_store_list(frame, chunks[1]),
            Screen::Menu => self.render_menu(frame, chunks[1]),
            Screen::Combo => self.render_combo(frame, chunks[1]),
            Screen::Mods => self.render_mods(frame, chunks[1]),
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
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1)])
            .split(area);
        let hint = Paragraph::new(Span::styled(
            self.current_hint(),
            Style::default().fg(Color::DarkGray),
        ));
        frame.render_widget(hint, chunks[0]);
        let color = match self.status_kind {
            StatusKind::Info => Color::Gray,
            StatusKind::Error => Color::Red,
        };
        let msg = truncate_width(&self.status, chunks[1].width as usize);
        let p = Paragraph::new(Span::styled(msg, Style::default().fg(color)));
        frame.render_widget(p, chunks[1]);
    }

    fn current_hint(&self) -> String {
        match self.screen {
            Screen::StoreSearch => "Tab 切换输入 · Enter 搜索 · q 退出".to_string(),
            Screen::StoreList => "↑↓/鼠标 选择 · Enter 进入菜单 · Esc 返回".to_string(),
            Screen::Menu => match self.menu_focus {
                MenuFocus::Search => "输入过滤 · Enter 确认 · Esc 退出搜索".to_string(),
                MenuFocus::Category => "↑↓ 选分类 · Enter/→ 到餐品 · ←→/Tab 切栏".to_string(),
                MenuFocus::Items => "↑↓ 选餐品 · Enter 加购 · Tab/←→ 切栏".to_string(),
                MenuFocus::Basket => "↑↓ 选择 · d 删除 · p 计价 · c 清空".to_string(),
            },
            Screen::Combo => "空格 选/切 · o 完成选餐 · Esc 取消".to_string(),
            Screen::Mods => "空格 切换 · Enter 确认 · Esc 取消".to_string(),
            Screen::Price => "↑↓ 选取餐方式 · Enter 下单 · Esc 返回".to_string(),
            Screen::OrderResult => "q 退出".to_string(),
        }
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
        // 背景：门店搜索页
        self.render_store_search(frame, area);

        let popup = centered_rect(72, 82, area);
        frame.render_widget(Clear, popup);
        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
            .split(popup);
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
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" 找到 {} 家门店", self.stores.len()),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ))),
            inner[0],
        );
        let list = List::new(items)
            .block(Block::default().title(" 选择门店 ").borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        frame.render_stateful_widget(list, inner[1], &mut self.store_state);
        frame.render_widget(
            Paragraph::new(Span::styled(
                "↑↓ 选择 · Enter 进入菜单 · Esc 返回",
                Style::default().fg(Color::Gray),
            )),
            inner[2],
        );
    }

    fn render_menu(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(5), Constraint::Min(1)])
            .split(area);

        // 顶部搜索框（3 行内容 + 边框，醒目）
        let search_style = if self.menu_focus == MenuFocus::Search {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let input_style = if self.menu_focus == MenuFocus::Search {
            Style::default().fg(Color::Yellow).bg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let search = Paragraph::new(vec![
            Line::from(Span::styled(
                "  🔍 搜索菜单",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    format!(
                        "{}{}",
                        self.menu_search,
                        if self.menu_focus == MenuFocus::Search {
                            "▎"
                        } else {
                            ""
                        }
                    ),
                    input_style,
                ),
            ]),
            Line::from(Span::styled(
                "  输入实时过滤 · Enter 确认 · Tab/←→ 切换栏",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(
            Block::default()
                .title(" 搜索 ")
                .borders(Borders::ALL)
                .border_style(search_style),
        );
        frame.render_widget(search, chunks[0]);

        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(22),
                Constraint::Percentage(48),
                Constraint::Percentage(30),
            ])
            .split(chunks[1]);

        // 左栏：分类
        let cat_border = if self.menu_focus == MenuFocus::Category {
            Color::Yellow
        } else {
            Color::Gray
        };
        let mut cat_items = vec![ListItem::new("  全部 ")];
        for c in &self.menu_categories {
            cat_items.push(ListItem::new(format!("  {c} ")));
        }
        let cat_list = List::new(cat_items)
            .block(
                Block::default()
                    .title(" 分类 ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(cat_border)),
            )
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        frame.render_stateful_widget(cat_list, body[0], &mut self.menu_cat_state);

        // 中栏：过滤后的餐品
        let visible = self.visible_menu();
        let items_border = if self.menu_focus == MenuFocus::Items {
            Color::Yellow
        } else {
            Color::Gray
        };
        let items: Vec<ListItem> = visible
            .iter()
            .map(|&i| {
                let m = &self.menu[i];
                let tag = if m.with_order { " [随单购]" } else { "" };
                ListItem::new(vec![Line::from(vec![
                    Span::styled(format!(" {} ", m.name), Style::default().fg(Color::White)),
                    Span::styled(format!("¥{}", m.price), Style::default().fg(Color::Yellow)),
                    Span::styled(tag, Style::default().fg(Color::Magenta)),
                ])])
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .title(format!(" 餐品（{} 件） ", visible.len()))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(items_border)),
            )
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        frame.render_stateful_widget(list, body[1], &mut self.menu_state);

        // 右栏：购物篮（常驻）
        let basket_border = if self.menu_focus == MenuFocus::Basket {
            Color::Yellow
        } else {
            Color::Gray
        };
        let total: u32 = self.basket.iter().map(|b| b.qty).sum();
        let basket_block = Block::default()
            .title(format!(" 购物篮（{total} 件） "))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(basket_border));
        if self.basket.is_empty() {
            let p = Paragraph::new("空，去菜单加购吧\n\n选中购物篮后:\nd 删除 · p 计价 · c 清空")
                .block(basket_block)
                .wrap(Wrap { trim: true });
            frame.render_widget(p, body[2]);
        } else {
            let b_items: Vec<ListItem> = self
                .basket
                .iter()
                .map(|b| {
                    let mut lines = vec![Line::from(vec![
                        Span::styled(format!(" {} ", b.label), Style::default().fg(Color::White)),
                        Span::styled(format!("x{}", b.qty), Style::default().fg(Color::Yellow)),
                    ])];
                    if !b.detail.is_empty() {
                        for sub in b.detail.split(" + ") {
                            lines.push(Line::from(Span::styled(
                                format!("   · {sub}"),
                                Style::default().fg(Color::Gray),
                            )));
                        }
                    }
                    ListItem::new(lines)
                })
                .collect();
            let blist = List::new(b_items)
                .block(basket_block)
                .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
            frame.render_stateful_widget(blist, body[2], &mut self.basket_state);
        }
    }

    fn render_combo(&mut self, frame: &mut Frame, area: Rect) {
        // 背景：菜单页
        self.render_menu(frame, area);

        let popup = centered_rect(66, 80, area);
        frame.render_widget(Clear, popup);

        let detail = match &self.detail {
            Some(d) => d,
            None => {
                frame.render_widget(
                    Paragraph::new("加载中...").block(Block::default().borders(Borders::ALL)),
                    popup,
                );
                return;
            }
        };
        let rounds = &detail.rounds;
        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
            .split(popup);

        let flat = self.combo_flat();
        let items: Vec<ListItem> = flat
            .iter()
            .map(|(ri, ci)| match ci {
                None => ListItem::new(Line::from(Span::styled(
                    format!(
                        "  ── 轮次{}/{}【{}】 选{}~{}项 ──",
                        ri + 1,
                        rounds.len(),
                        rounds[*ri].name,
                        rounds[*ri].min_quantity,
                        rounds[*ri].max_quantity
                    ),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ))),
                Some(ci) => {
                    let choice = &rounds[*ri].choices[*ci];
                    let mark = if self.combo_toggles[*ri][*ci] { "■ " } else { "□ " };
                    let price = choice.diff_price.as_deref().unwrap_or("");
                    let modify = if choice.support_modify { " [可特调]" } else { "" };
                    ListItem::new(Line::from(vec![
                        Span::styled(mark, Style::default().fg(Color::Green)),
                        Span::styled(choice.name.clone(), Style::default().fg(Color::White)),
                        Span::styled(format!(" {price}"), Style::default().fg(Color::Yellow)),
                        Span::styled(modify, Style::default().fg(Color::Cyan)),
                    ]))
                }
            })
            .collect();
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {} ", detail.name),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ))),
            inner[0],
        );
        let p = List::new(items)
            .block(Block::default().title(" 选配（所有轮次） ").borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        frame.render_stateful_widget(p, inner[1], &mut self.combo_state);
        frame.render_widget(
            Paragraph::new(Span::styled(
                "空格 选/切 · Enter 单选 · o 完成选餐 · Esc 取消",
                Style::default().fg(Color::Gray),
            )),
            inner[2],
        );
    }

    fn render_mods(&mut self, frame: &mut Frame, area: Rect) {
        // 背景：选餐弹窗
        self.render_combo(frame, area);

        let popup = centered_rect(48, 48, area);
        frame.render_widget(Clear, popup);

        let Some(g) = self.mods_groups.get(self.mods_group_idx).cloned() else {
            frame.render_widget(
                Paragraph::new("特调完成").block(Block::default().borders(Borders::ALL)),
                popup,
            );
            return;
        };
        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
            .split(popup);
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
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " 特调选项 ",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ))),
            inner[0],
        );
        let list = List::new(items)
            .block(Block::default().title(title).borders(Borders::ALL))
            .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::Yellow));
        frame.render_stateful_widget(list, inner[1], &mut self.mods_state);
        frame.render_widget(
            Paragraph::new(Span::styled(hint, Style::default().fg(Color::Gray))),
            inner[2],
        );
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

fn build_mod_groups(modification: &Modification) -> (Vec<ModGroup>, Vec<Value>) {
    let mut groups: Vec<ModGroup> = Vec::new();
    let mut auto: Vec<Value> = Vec::new();
    for item in &modification.items {
        if item.values.is_empty() {
            continue;
        }
        let multi = item.values.iter().all(|v| v.unselected_key.is_some());
        if item.min_values == 1 && item.max_values == 1 && item.values.len() == 1 {
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
    (groups, auto)
}

fn mod_names_from_sel(modification: &Modification, sel: &ModSelection) -> Vec<String> {
    modification
        .items
        .iter()
        .flat_map(|item| &item.values)
        .filter(|v| {
            sel.values.iter().any(|sv| {
                sv.get("code").and_then(Value::as_str) == Some(v.code.as_str())
                    && sv.get("key").and_then(Value::as_str) == Some(v.selected_key.as_str())
            })
        })
        .map(|v| v.name.clone())
        .collect()
}

fn mod_names(choice: &Choice, sel: &ModSelection) -> Vec<String> {
    choice
        .modification
        .as_ref()
        .map(|m| {
            m.items
                .iter()
                .flat_map(|item| &item.values)
                .filter(|v| {
                    sel.values.iter().any(|sv| {
                        sv.get("code").and_then(Value::as_str) == Some(v.code.as_str())
                            && sv.get("key").and_then(Value::as_str) == Some(v.selected_key.as_str())
                    })
                })
                .map(|v| v.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn content_area(area: Rect) -> Rect {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(area);
    chunks[1]
}

fn menu_panes(area: Rect) -> [Rect; 3] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(1)])
        .split(area);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(22),
            Constraint::Percentage(48),
            Constraint::Percentage(30),
        ])
        .split(chunks[1]);
    [body[0], body[1], body[2]]
}

fn list_index_at(list_rect: Rect, row: u16, offset: usize, len: usize) -> Option<usize> {
    if row < list_rect.y + 1 {
        return None;
    }
    let idx = (row - list_rect.y - 1) as usize + offset;
    if idx < len {
        Some(idx)
    } else {
        None
    }
}

fn truncate_width(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0usize;
    for c in s.chars() {
        let cw = c.width().unwrap_or(1);
        if w + cw > width {
            out.push('…');
            return out;
        }
        out.push(c);
        w += cw;
    }
    out
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}