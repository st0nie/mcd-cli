mod config;
mod fmt;
mod mcp;
mod meal;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use mcp::McpClient;
use serde_json::Value;
use std::io::{self, Write};

#[derive(Parser, Debug)]
#[command(name = "mcd-cli")]
#[command(about = "麦当劳 MCP CLI - 基于麦当劳官方 MCP Server 的点餐工具")]
struct Cli {
    #[arg(long, env = "MCD_MCP_TOKEN", help = "MCP Token")]
    token: Option<String>,

    #[arg(
        long,
        env = "MCD_MCP_URL",
        default_value = "https://mcp.mcd.cn",
        help = "MCP Server URL"
    )]
    url: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// 初始化并测试连接
    Init,
    /// 查看活动日历
    Calendar,
    /// 查看当前时间
    Time,
    /// 查看我的账户（含积分）
    Account,
    /// 查询附近门店
    Nearby {
        #[arg(long, default_value = "1", help = "beType: 1=到店自取, 5=得来速")]
        be_type: i32,
        #[arg(long, default_value = "2", help = "searchType: 1=收藏餐厅, 2=按位置")]
        search_type: i32,
        #[arg(long, help = "城市名")]
        city: Option<String>,
        #[arg(long, help = "关键词")]
        keyword: Option<String>,
    },
    /// 地址管理
    Address {
        #[command(subcommand)]
        action: AddressCommands,
    },
    /// 外送/团餐可配送门店查询
    DeliveryStores {
        #[arg(long, help = "地址 ID")]
        address_id: String,
        #[arg(long, default_value = "2", help = "beType: 2=麦乐送, 6=团餐")]
        be_type: i32,
    },
    /// 查询助餐服务（团餐场景）
    Catering {
        #[arg(long, help = "门店 storeCode")]
        store: String,
        #[arg(long, help = "门店 beCode")]
        be: String,
        #[arg(long, help = "预约时间，格式 yyyy-MM-dd HH:mm")]
        reservation_date: Option<String>,
    },
    /// 餐品营养信息
    Nutrition,
    /// 浏览菜单
    Menu {
        #[arg(long, help = "门店 storeCode")]
        store: String,
        #[arg(long, help = "门店 beCode（外送必填）")]
        be: Option<String>,
        #[arg(long, default_value = "1", help = "订单类型: 1=到店取餐, 2=外送")]
        order_type: i32,
        #[arg(
            long,
            default_value = "1",
            help = "业务类型: 1=到店取餐, 2=麦乐送, 5=得来速, 6=团餐"
        )]
        be_type: i32,
        #[arg(long, help = "预约时间，格式 yyyy-MM-dd HH:mm")]
        reservation_date: Option<String>,
    },
    /// 餐品详情
    Detail {
        #[arg(help = "商品 code")]
        code: String,
        #[arg(long, help = "门店 storeCode")]
        store: String,
        #[arg(long, help = "门店 beCode（外送必填）")]
        be: Option<String>,
        #[arg(long, default_value = "1", help = "订单类型: 1=到店取餐, 2=外送")]
        order_type: i32,
        #[arg(
            long,
            default_value = "1",
            help = "业务类型: 1=到店取餐, 2=麦乐送, 5=得来速, 6=团餐"
        )]
        be_type: i32,
        #[arg(long, help = "预约时间，格式 yyyy-MM-dd HH:mm")]
        reservation_date: Option<String>,
        #[arg(long, help = "展开特调选项")]
        mods: bool,
    },
    /// 选餐定制（选配+特调，生成下单 items JSON）
    Select {
        #[arg(help = "商品 code")]
        code: String,
        #[arg(long, help = "门店 storeCode")]
        store: String,
        #[arg(long, help = "门店 beCode（外送必填）")]
        be: Option<String>,
        #[arg(long, default_value = "1", help = "订单类型: 1=到店取餐, 2=外送")]
        order_type: i32,
        #[arg(
            long,
            default_value = "1",
            help = "业务类型: 1=到店取餐, 2=麦乐送, 5=得来速, 6=团餐"
        )]
        be_type: i32,
        #[arg(long, help = "预约时间，格式 yyyy-MM-dd HH:mm")]
        reservation_date: Option<String>,
        #[arg(long, default_value = "1", help = "数量")]
        qty: u32,
        #[arg(long, help = "非交互选配，格式 1=1600,2=3050（轮次序号=商品code）")]
        pick: Option<String>,
        #[arg(long, help = "只输出 items JSON 到 stdout（供管道使用）")]
        json: bool,
    },
    /// 优惠券
    Coupon {
        #[command(subcommand)]
        action: CouponCommands,
    },
    /// 积分商城
    Mall {
        #[command(subcommand)]
        action: MallCommands,
    },
    /// 计算价格
    Price {
        #[arg(long, help = "门店 storeCode")]
        store: String,
        #[arg(long, help = "门店 beCode（外送必填）")]
        be: Option<String>,
        #[arg(long, default_value = "1", help = "订单类型: 1=到店取餐, 2=外送")]
        order_type: i32,
        #[arg(
            long,
            default_value = "1",
            help = "业务类型: 1=到店取餐, 2=麦乐送, 5=得来速, 6=团餐"
        )]
        be_type: i32,
        #[arg(long, help = "预约时间，格式 yyyy-MM-dd HH:mm")]
        reservation_date: Option<String>,
        #[arg(long, help = "优惠券 ID")]
        coupon_id: Option<String>,
        #[arg(long, help = "优惠券编码")]
        coupon_code: Option<String>,
        #[arg(
            long,
            help = "商品列表 JSON，如 [{\"productCode\":\"xxx\",\"quantity\":1}]"
        )]
        items: String,
    },
    /// 创建订单
    Order {
        #[command(subcommand)]
        action: OrderCommands,
    },
    /// 登录并保存 Token 到配置文件
    Login {
        #[arg(long, help = "MCP Token")]
        token: String,
        #[arg(long, help = "MCP Server URL")]
        url: Option<String>,
    },
    /// 查看配置文件路径和当前配置
    Config,
    /// 交互式点单模式
    Interactive,
}

#[derive(Subcommand, Debug)]
enum AddressCommands {
    /// 查询配送地址
    List {
        #[arg(long, default_value = "2", help = "beType: 2=麦乐送, 6=团餐")]
        be_type: i32,
    },
    /// 新增配送地址
    Add {
        #[arg(long, default_value = "2", help = "beType: 2=麦乐送, 6=团餐")]
        be_type: i32,
    },
}

#[derive(Subcommand, Debug)]
enum CouponCommands {
    /// 门店可用优惠券
    Store {
        #[arg(long, help = "门店 storeCode")]
        store: String,
        #[arg(long, help = "门店 beCode")]
        be: String,
        #[arg(long, default_value = "1", help = "订单类型: 1=到店取餐, 2=外送")]
        order_type: i32,
        #[arg(
            long,
            default_value = "1",
            help = "业务类型: 1=到店取餐, 2=麦乐送, 5=得来速, 6=团餐"
        )]
        be_type: i32,
        #[arg(long, help = "预约时间，格式 yyyy-MM-dd HH:mm")]
        reservation_date: Option<String>,
    },
    /// 我的优惠券
    My,
    /// 可领优惠券列表
    Available,
    /// 一键领券
    Receive,
}

#[derive(Subcommand, Debug)]
enum MallCommands {
    /// 积分兑换商品列表
    Products {
        #[arg(long, help = "类目筛选，如 1>4,2")]
        cat_rule_ids: Option<String>,
    },
    /// 积分兑换商品详情
    Detail {
        #[arg(help = "商品 spuId")]
        spu_id: i64,
    },
    /// 积分兑换券下单（虚拟商品）
    Exchange {
        #[arg(long, help = "商品 skuId")]
        sku_id: i64,
        #[arg(long, default_value = "1", help = "兑换数量")]
        count: i32,
    },
    /// 积分兑换实物下单
    Physical {
        #[arg(long, help = "商品 skuId")]
        sku_id: i64,
        #[arg(long, default_value = "1", help = "兑换数量")]
        count: i32,
        #[arg(long, help = "配送地址 ID")]
        address_id: String,
        #[arg(
            long,
            default_value = "2",
            help = "spuCategory: 1=虚拟商品, 2=实体物品"
        )]
        spu_category: String,
    },
    /// 商城订单查询
    Orders {
        #[arg(long, help = "最后一个订单 ID（翻页用）")]
        last_id: Option<i64>,
        #[arg(long, default_value = "10", help = "查询数量")]
        size: i32,
    },
    /// 商城订单详情
    OrderDetail {
        #[arg(help = "订单 ID")]
        order_id: String,
    },
}

#[derive(Subcommand, Debug)]
enum OrderCommands {
    /// 创建订单
    Create {
        #[arg(long, help = "门店 storeCode")]
        store: String,
        #[arg(long, help = "门店 beCode（外送必填）")]
        be: Option<String>,
        #[arg(long, help = "地址 ID（外送必填）")]
        address: Option<String>,
        #[arg(long, help = "商品列表 JSON")]
        items: String,
        #[arg(long, default_value = "1", help = "订单类型: 1=到店取餐, 2=外送")]
        order_type: i32,
        #[arg(
            long,
            default_value = "1",
            help = "业务类型: 1=到店取餐, 2=麦乐送, 5=得来速, 6=团餐"
        )]
        be_type: i32,
        #[arg(long, help = "取餐方式编码（到店/得来速必填，从 price 获取）")]
        take_way: Option<String>,
        #[arg(long, help = "预约时间，格式 yyyy-MM-dd HH:mm")]
        reservation_date: Option<String>,
        #[arg(long, help = "优惠券 ID")]
        coupon_id: Option<String>,
        #[arg(long, help = "优惠券编码")]
        coupon_code: Option<String>,
        #[arg(long, help = "助餐服务 code（团餐必填）")]
        gm_service_code: Option<String>,
    },
    /// 查询订单
    Query {
        #[arg(help = "订单号 orderId")]
        id: String,
    },
}

fn read_line(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

fn read_line_opt(prompt: &str) -> Option<String> {
    let s = read_line(prompt).ok()?;
    if s.is_empty() { None } else { Some(s) }
}

fn print_divider() {
    println!("{}", "-".repeat(50));
}

fn print_result(result: &mcp::ToolResult) {
    if let Some(ref structured) = result.structured_content {
        fmt::pretty_print(structured);
        return;
    }
    let text = result
        .content
        .iter()
        .filter_map(|c| c.text.clone())
        .collect::<Vec<_>>()
        .join("");
    fmt::pretty_print(&Value::String(text));
}

async fn run_init(client: &McpClient) -> Result<()> {
    println!("正在连接麦当劳 MCP Server...");
    let result = client.initialize().await?;
    println!("✅ 连接成功!");
    println!("   协议版本: {}", result.protocol_version);
    println!(
        "   服务端: {} v{}",
        result.server_info.name, result.server_info.version
    );
    Ok(())
}

async fn run_calendar(client: &McpClient) -> Result<()> {
    let result = client
        .call_tool("campaign-calendar", serde_json::json!({}))
        .await?;
    print_result(&result);
    Ok(())
}

async fn run_time(client: &McpClient) -> Result<()> {
    let result = client
        .call_tool("now-time-info", serde_json::json!({}))
        .await?;
    print_result(&result);
    Ok(())
}

async fn run_account(client: &McpClient) -> Result<()> {
    let result = client
        .call_tool("query-my-account", serde_json::json!({}))
        .await?;
    print_result(&result);
    Ok(())
}

async fn run_nearby(
    client: &McpClient,
    be_type: i32,
    search_type: i32,
    city: Option<&str>,
    keyword: Option<&str>,
) -> Result<()> {
    let mut args = serde_json::json!({
        "beType": be_type,
        "searchType": search_type,
    });
    if let Some(c) = city {
        args["city"] = Value::String(c.to_string());
    }
    if let Some(k) = keyword {
        args["keyword"] = Value::String(k.to_string());
    }
    let result = client.call_tool("query-nearby-stores", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_address_list(client: &McpClient, be_type: i32) -> Result<()> {
    let result = client
        .call_tool(
            "delivery-query-addresses",
            serde_json::json!({"beType": be_type}),
        )
        .await?;
    print_result(&result);
    Ok(())
}

async fn run_address_add(client: &McpClient, be_type: i32) -> Result<()> {
    println!("请输入配送地址信息（beType={}）:", be_type);
    let city = read_line("城市名称: ")?;
    let contact_name = read_line("联系人姓名: ")?;
    let phone = read_line("联系电话: ")?;
    let address = read_line("配送地址（小区/楼栋）: ")?;
    let address_detail = read_line("门牌号: ")?;
    let gender = read_line_opt("性别（先生/女士，回车跳过）: ");

    let mut args = serde_json::json!({
        "city": city,
        "contactName": contact_name,
        "phone": phone,
        "address": address,
        "addressDetail": address_detail,
        "beType": be_type
    });
    if let Some(g) = gender {
        args["gender"] = Value::String(g);
    }

    let result = client.call_tool("delivery-create-address", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_delivery_stores(client: &McpClient, address_id: &str, be_type: i32) -> Result<()> {
    let args = serde_json::json!({
        "addressId": address_id,
        "beType": be_type,
    });
    let result = client.call_tool("delivery-query-stores", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_catering(
    client: &McpClient,
    store: &str,
    be: &str,
    reservation_date: Option<&str>,
) -> Result<()> {
    let mut args = serde_json::json!({
        "storeCode": store,
        "beCode": be,
        "beType": 6,
        "orderType": 2,
    });
    if let Some(r) = reservation_date {
        args["reservationDate"] = Value::String(r.to_string());
    }
    let result = client.call_tool("query-meal-assistance", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_nutrition(client: &McpClient) -> Result<()> {
    let result = client
        .call_tool("list-nutrition-foods", serde_json::json!({}))
        .await?;
    print_result(&result);
    Ok(())
}

async fn run_menu(
    client: &McpClient,
    store: &str,
    be: Option<&str>,
    order_type: i32,
    be_type: i32,
    reservation_date: Option<&str>,
) -> Result<()> {
    let mut args = serde_json::json!({
        "storeCode": store,
        "orderType": order_type,
        "beType": be_type,
    });
    if let Some(b) = be {
        args["beCode"] = Value::String(b.to_string());
    }
    if let Some(r) = reservation_date {
        args["reservationDate"] = Value::String(r.to_string());
    }
    let result = client.call_tool("query-meals", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_detail(
    client: &McpClient,
    code: &str,
    store: &str,
    be: Option<&str>,
    order_type: i32,
    be_type: i32,
    reservation_date: Option<&str>,
) -> Result<()> {
    let mut args = serde_json::json!({
        "code": code,
        "storeCode": store,
        "orderType": order_type,
        "beType": be_type,
    });
    if let Some(b) = be {
        args["beCode"] = Value::String(b.to_string());
    }
    if let Some(r) = reservation_date {
        args["reservationDate"] = Value::String(r.to_string());
    }
    let result = client.call_tool("query-meal-detail", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_coupon_store(
    client: &McpClient,
    store: &str,
    be: &str,
    order_type: i32,
    be_type: i32,
    reservation_date: Option<&str>,
) -> Result<()> {
    let mut args = serde_json::json!({
        "storeCode": store,
        "beCode": be,
        "orderType": order_type,
        "beType": be_type,
    });
    if let Some(r) = reservation_date {
        args["reservationDate"] = Value::String(r.to_string());
    }
    let result = client.call_tool("query-store-coupons", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_coupon_my(client: &McpClient) -> Result<()> {
    let result = client
        .call_tool("query-my-coupons", serde_json::json!({}))
        .await?;
    print_result(&result);
    Ok(())
}

async fn run_coupon_available(client: &McpClient) -> Result<()> {
    let result = client
        .call_tool("available-coupons", serde_json::json!({}))
        .await?;
    print_result(&result);
    Ok(())
}

async fn run_coupon_receive(client: &McpClient) -> Result<()> {
    let result = client
        .call_tool("auto-bind-coupons", serde_json::json!({}))
        .await?;
    print_result(&result);
    Ok(())
}

async fn run_mall_products(client: &McpClient, cat_rule_ids: Option<&str>) -> Result<()> {
    let mut args = serde_json::json!({});
    if let Some(c) = cat_rule_ids {
        args["catRuleIds"] = Value::String(c.to_string());
    }
    let result = client.call_tool("mall-points-products", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_mall_detail(client: &McpClient, spu_id: i64) -> Result<()> {
    let args = serde_json::json!({"spuId": spu_id});
    let result = client.call_tool("mall-product-detail", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_mall_exchange(client: &McpClient, sku_id: i64, count: i32) -> Result<()> {
    let args = serde_json::json!({"skuId": sku_id, "count": count});
    let result = client.call_tool("mall-create-order", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_mall_physical(
    client: &McpClient,
    sku_id: i64,
    count: i32,
    address_id: &str,
    spu_category: &str,
) -> Result<()> {
    let args = serde_json::json!({
        "skuId": sku_id,
        "count": count,
        "addressId": address_id,
        "spuCategory": spu_category,
    });
    let result = client.call_tool("mall-create-physical-order", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_mall_orders(client: &McpClient, last_id: Option<i64>, size: i32) -> Result<()> {
    let mut args = serde_json::json!({});
    if let Some(l) = last_id {
        args["lastId"] = Value::Number(l.into());
    }
    if size > 0 {
        args["size"] = Value::Number(size.into());
    }
    let result = client.call_tool("mall-order-list", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_mall_order_detail(client: &McpClient, order_id: &str) -> Result<()> {
    let args = serde_json::json!({"orderId": order_id});
    let result = client.call_tool("mall-order-detail", args).await?;
    print_result(&result);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_calculate_price(
    client: &McpClient,
    store: &str,
    be: Option<&str>,
    order_type: i32,
    be_type: i32,
    reservation_date: Option<&str>,
    coupon_id: Option<&str>,
    coupon_code: Option<&str>,
    items: &str,
) -> Result<()> {
    let items_val: Value = serde_json::from_str(items)?;
    let mut args = serde_json::json!({
        "storeCode": store,
        "orderType": order_type,
        "beType": be_type,
        "items": items_val,
    });
    if let Some(b) = be {
        args["beCode"] = Value::String(b.to_string());
    }
    if let Some(r) = reservation_date {
        args["reservationDate"] = Value::String(r.to_string());
    }
    if let Some(c) = coupon_id {
        args["couponId"] = Value::String(c.to_string());
    }
    if let Some(c) = coupon_code {
        args["couponCode"] = Value::String(c.to_string());
    }
    let result = client.call_tool("calculate-price", args).await?;
    print_result(&result);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_order_create(
    client: &McpClient,
    store: &str,
    be: Option<&str>,
    address: Option<&str>,
    items: &str,
    order_type: i32,
    be_type: i32,
    take_way: Option<&str>,
    reservation_date: Option<&str>,
    coupon_id: Option<&str>,
    coupon_code: Option<&str>,
    gm_service_code: Option<&str>,
) -> Result<()> {
    let items_val: Value = serde_json::from_str(items)?;
    let mut args = serde_json::json!({
        "storeCode": store,
        "items": items_val,
        "orderType": order_type,
        "beType": be_type,
    });
    if let Some(b) = be {
        args["beCode"] = Value::String(b.to_string());
    }
    if let Some(a) = address {
        args["addressId"] = Value::String(a.to_string());
    }
    if let Some(tw) = take_way {
        args["takeWayCode"] = Value::String(tw.to_string());
    }
    if let Some(r) = reservation_date {
        args["reservationDate"] = Value::String(r.to_string());
    }
    if let Some(c) = coupon_id {
        args["couponId"] = Value::String(c.to_string());
    }
    if let Some(c) = coupon_code {
        args["couponCode"] = Value::String(c.to_string());
    }
    if let Some(g) = gm_service_code {
        args["gmServiceCode"] = Value::String(g.to_string());
    }
    let result = client.call_tool("create-order", args).await?;
    print_result(&result);
    Ok(())
}

async fn run_order_query(client: &McpClient, id: &str) -> Result<()> {
    let args = serde_json::json!({"orderId": id});
    let result = client.call_tool("query-order", args).await?;
    print_result(&result);
    Ok(())
}

async fn fetch_meal_detail(
    client: &McpClient,
    code: &str,
    store: &str,
    be: Option<&str>,
    order_type: i32,
    be_type: i32,
    reservation_date: Option<&str>,
) -> Result<meal::MealDetail> {
    let mut args = serde_json::json!({
        "code": code,
        "storeCode": store,
        "orderType": order_type,
        "beType": be_type,
    });
    if let Some(b) = be {
        args["beCode"] = Value::String(b.to_string());
    }
    if let Some(r) = reservation_date {
        args["reservationDate"] = Value::String(r.to_string());
    }
    let result = client.call_tool("query-meal-detail", args).await?;

    let data: Value = if let Some(ref structured) = result.structured_content {
        structured
            .get("data")
            .cloned()
            .unwrap_or_else(|| structured.clone())
    } else {
        let text = result
            .content
            .iter()
            .filter_map(|c| c.text.clone())
            .collect::<Vec<_>>()
            .join("");
        let mut data = Value::Null;
        if let Some(idx) = text.find("{\"success\":")
            && let Ok(json) = serde_json::from_str::<Value>(&text[idx..])
        {
            data = json.get("data").cloned().unwrap_or(Value::Null);
        }
        data
    };
    meal::parse_detail(&data)
}

type SelectionPlan = (
    Vec<Vec<usize>>,
    Vec<Vec<Option<meal::ModSelection>>>,
    Option<meal::ModSelection>,
);

fn interactively_select(detail: &meal::MealDetail, qty: u32) -> Result<Value> {
    let stdin = io::stdin();
    let mut ui = meal::Ui {
        reader: &mut stdin.lock(),
        writer: &mut io::stdout(),
    };
    print_divider();
    println!("🔧 选餐: {} ({})", detail.name, detail.code);
    print_divider();
    let (picks, mods, product_mods): SelectionPlan = if detail.rounds.is_empty() {
        let product_mods = meal::pick_product_mods(&mut ui, detail)?;
        (Vec::new(), Vec::new(), product_mods)
    } else {
        let mut picks = Vec::new();
        let mut mods = Vec::new();
        for (i, round) in detail.rounds.iter().enumerate() {
            let round_picks = meal::pick_round_choices(&mut ui, round, i + 1)?;
            let round_mods = round_picks
                .iter()
                .map(|&idx| meal::pick_mods(&mut ui, &round.choices[idx]))
                .collect::<Result<Vec<_>>>()?;
            picks.push(round_picks);
            mods.push(round_mods);
        }
        (picks, mods, None)
    };
    let item = meal::build_item_value(detail, qty, &picks, &mods, product_mods);
    println!("{}", meal::render_selection_summary(detail, &picks, &mods));
    Ok(item)
}

#[allow(clippy::too_many_arguments)]
async fn run_select(
    client: &McpClient,
    code: &str,
    store: &str,
    be: Option<&str>,
    order_type: i32,
    be_type: i32,
    reservation_date: Option<&str>,
    qty: u32,
    pick: Option<&str>,
    json_only: bool,
) -> Result<()> {
    let detail = fetch_meal_detail(
        client,
        code,
        store,
        be,
        order_type,
        be_type,
        reservation_date,
    )
    .await?;

    let (item, summary): (Value, String) = if let Some(spec) = pick {
        let picks = meal::parse_pick_spec(&detail, spec)?;
        let mods: Vec<Vec<Option<meal::ModSelection>>> = picks
            .iter()
            .map(|round_picks| round_picks.iter().map(|_| None).collect())
            .collect();
        let item = meal::build_item_value(&detail, qty, &picks, &mods, None);
        (item, meal::render_selection_summary(&detail, &picks, &mods))
    } else {
        (interactively_select(&detail, qty)?, String::new())
    };

    if json_only {
        println!("{}", serde_json::json!([item]));
    } else {
        print_divider();
        println!("🛒 选购清单: {} ({})", detail.name, detail.code);
        if summary.is_empty() {
            println!("  {}", detail.name);
        } else {
            println!("{}", summary);
        }
        println!("\n📦 已生成 items JSON:");
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!([item]))?
        );
        println!("提示: 可复制到 price / order create 的 --items，或进入交互模式继续点单");
    }
    Ok(())
}

async fn interactive_mode(client: &McpClient) -> Result<()> {
    loop {
        print_divider();
        println!("🍟 麦当劳 CLI 交互模式");
        print_divider();
        println!("1.  测试连接");
        println!("2.  查看活动日历");
        println!("3.  查看当前时间");
        println!("4.  查看我的账户/积分");
        println!("5.  查询附近门店（到店/得来速）");
        println!("6.  外送/团餐可配送门店");
        println!("7.  查询助餐服务（团餐）");
        println!("8.  餐品营养信息");
        println!("9.  查看配送地址");
        println!("10. 新增配送地址");
        println!("11. 浏览菜单");
        println!("12. 查看餐品详情");
        println!("13. 门店优惠券");
        println!("14. 我的优惠券 / 一键领券 / 可领券");
        println!("15. 积分商城");
        println!("16. 快速点单（选餐/特调 → 计价 → 下单）");
        println!("17. 查询订单详情");
        println!("0.  退出");
        print_divider();

        let choice = read_line("请输入选项: ")?;
        match choice.as_str() {
            "1" => {
                if let Err(e) = run_init(client).await {
                    println!("❌ 连接失败: {}", e);
                }
            }
            "2" => {
                if let Err(e) = run_calendar(client).await {
                    println!("❌ 查询失败: {}", e);
                }
            }
            "3" => {
                if let Err(e) = run_time(client).await {
                    println!("❌ 查询失败: {}", e);
                }
            }
            "4" => {
                if let Err(e) = run_account(client).await {
                    println!("❌ 查询失败: {}", e);
                }
            }
            "5" => {
                let bt = read_line("beType (1=到店自取, 5=得来速): ")?;
                let be_type: i32 = bt.parse().unwrap_or(1);
                let city = read_line("城市名: ")?;
                let keyword = read_line("关键词（商圈/学校/路名）: ")?;
                if let Err(e) = run_nearby(client, be_type, 2, Some(&city), Some(&keyword)).await {
                    println!("❌ 查询失败: {}", e);
                }
            }
            "6" => {
                let aid = read_line("地址 ID: ")?;
                let bt = read_line("beType (2=麦乐送, 6=团餐): ")?;
                let be_type: i32 = bt.parse().unwrap_or(2);
                if let Err(e) = run_delivery_stores(client, &aid, be_type).await {
                    println!("❌ 查询失败: {}", e);
                }
            }
            "7" => {
                let store = read_line("门店 storeCode: ")?;
                let be = read_line("门店 beCode: ")?;
                let rd = read_line_opt("预约时间（yyyy-MM-dd HH:mm，回车跳过）: ");
                if let Err(e) = run_catering(client, &store, &be, rd.as_deref()).await {
                    println!("❌ 查询失败: {}", e);
                }
            }
            "8" => {
                if let Err(e) = run_nutrition(client).await {
                    println!("❌ 查询失败: {}", e);
                }
            }
            "9" => {
                let bt = read_line("beType (2=麦乐送, 6=团餐，默认2）: ")?;
                let be_type: i32 = if bt.is_empty() {
                    2
                } else {
                    bt.parse().unwrap_or(2)
                };
                if let Err(e) = run_address_list(client, be_type).await {
                    println!("❌ 查询失败: {}", e);
                }
            }
            "10" => {
                let bt = read_line("beType (2=麦乐送, 6=团餐，默认2）: ")?;
                let be_type: i32 = if bt.is_empty() {
                    2
                } else {
                    bt.parse().unwrap_or(2)
                };
                if let Err(e) = run_address_add(client, be_type).await {
                    println!("❌ 添加失败: {}", e);
                }
            }
            "11" => {
                let store = read_line("门店 storeCode: ")?;
                let be = read_line_opt("门店 beCode（到店/得来速回车跳过）: ");
                let ot = read_line("订单类型 (1=到店, 2=外送): ")?;
                let order_type: i32 = ot.parse().unwrap_or(1);
                let bt = read_line("beType (1=到店, 2=麦乐送, 5=得来速, 6=团餐，默认1）: ")?;
                let be_type: i32 = if bt.is_empty() {
                    1
                } else {
                    bt.parse().unwrap_or(1)
                };
                let rd = read_line_opt("预约时间（回车跳过）: ");
                if let Err(e) = run_menu(
                    client,
                    &store,
                    be.as_deref(),
                    order_type,
                    be_type,
                    rd.as_deref(),
                )
                .await
                {
                    println!("❌ 查询失败: {}", e);
                }
            }
            "12" => {
                let code = read_line("商品 code: ")?;
                let store = read_line("门店 storeCode: ")?;
                let be = read_line_opt("门店 beCode（到店/得来速回车跳过）: ");
                let ot = read_line("订单类型 (1=到店, 2=外送): ")?;
                let order_type: i32 = ot.parse().unwrap_or(1);
                let bt = read_line("beType (1=到店, 2=麦乐送, 5=得来速, 6=团餐，默认1）: ")?;
                let be_type: i32 = if bt.is_empty() {
                    1
                } else {
                    bt.parse().unwrap_or(1)
                };
                let rd = read_line_opt("预约时间（回车跳过）: ");
                if let Err(e) = run_detail(
                    client,
                    &code,
                    &store,
                    be.as_deref(),
                    order_type,
                    be_type,
                    rd.as_deref(),
                )
                .await
                {
                    println!("❌ 查询失败: {}", e);
                }
            }
            "13" => {
                let store = read_line("门店 storeCode: ")?;
                let be = read_line("门店 beCode: ")?;
                let ot = read_line("订单类型 (1=到店, 2=外送): ")?;
                let order_type: i32 = ot.parse().unwrap_or(1);
                let bt = read_line("beType (1=到店, 2=麦乐送, 5=得来速, 6=团餐，默认1）: ")?;
                let be_type: i32 = if bt.is_empty() {
                    1
                } else {
                    bt.parse().unwrap_or(1)
                };
                let rd = read_line_opt("预约时间（回车跳过）: ");
                if let Err(e) =
                    run_coupon_store(client, &store, &be, order_type, be_type, rd.as_deref()).await
                {
                    println!("❌ 查询失败: {}", e);
                }
            }
            "14" => {
                println!("a. 我的优惠券");
                println!("b. 可领优惠券列表");
                println!("c. 一键领券");
                let sub = read_line("选择: ")?;
                match sub.as_str() {
                    "a" => {
                        if let Err(e) = run_coupon_my(client).await {
                            println!("❌ 查询失败: {}", e);
                        }
                    }
                    "b" => {
                        if let Err(e) = run_coupon_available(client).await {
                            println!("❌ 查询失败: {}", e);
                        }
                    }
                    "c" => {
                        if let Err(e) = run_coupon_receive(client).await {
                            println!("❌ 领券失败: {}", e);
                        }
                    }
                    _ => println!("无效选择"),
                }
            }
            "15" => {
                println!("a. 积分兑换商品列表");
                println!("b. 积分兑换商品详情");
                println!("c. 积分兑换券下单（虚拟）");
                println!("d. 积分兑换实物下单");
                println!("e. 商城订单查询");
                println!("f. 商城订单详情");
                let sub = read_line("选择: ")?;
                match sub.as_str() {
                    "a" => {
                        let cat = read_line_opt("类目筛选（回车跳过，如 1>4,2）: ");
                        if let Err(e) = run_mall_products(client, cat.as_deref()).await {
                            println!("❌ 查询失败: {}", e);
                        }
                    }
                    "b" => {
                        let spu = read_line("商品 spuId: ")?;
                        let spu_id: i64 = spu.parse().unwrap_or(0);
                        if let Err(e) = run_mall_detail(client, spu_id).await {
                            println!("❌ 查询失败: {}", e);
                        }
                    }
                    "c" => {
                        let sku = read_line("商品 skuId: ")?;
                        let sku_id: i64 = sku.parse().unwrap_or(0);
                        let cnt = read_line("兑换数量（默认1）: ")?;
                        let count: i32 = cnt.parse().unwrap_or(1);
                        if let Err(e) = run_mall_exchange(client, sku_id, count).await {
                            println!("❌ 兑换失败: {}", e);
                        }
                    }
                    "d" => {
                        let sku = read_line("商品 skuId: ")?;
                        let sku_id: i64 = sku.parse().unwrap_or(0);
                        let cnt = read_line("兑换数量（默认1）: ")?;
                        let count: i32 = cnt.parse().unwrap_or(1);
                        let aid = read_line("配送地址 ID: ")?;
                        let sc = read_line("spuCategory (1=虚拟, 2=实体，默认2）: ")?;
                        let spu_category = if sc.is_empty() { "2".to_string() } else { sc };
                        if let Err(e) =
                            run_mall_physical(client, sku_id, count, &aid, &spu_category).await
                        {
                            println!("❌ 兑换失败: {}", e);
                        }
                    }
                    "e" => {
                        let lid = read_line_opt("lastId（翻页用，回车跳过）: ");
                        let sz = read_line("查询数量（默认10）: ")?;
                        let size: i32 = if sz.is_empty() {
                            10
                        } else {
                            sz.parse().unwrap_or(10)
                        };
                        let last_id = lid.and_then(|s| s.parse().ok());
                        if let Err(e) = run_mall_orders(client, last_id, size).await {
                            println!("❌ 查询失败: {}", e);
                        }
                    }
                    "f" => {
                        let oid = read_line("订单 ID: ")?;
                        if let Err(e) = run_mall_order_detail(client, &oid).await {
                            println!("❌ 查询失败: {}", e);
                        }
                    }
                    _ => println!("无效选择"),
                }
            }
            "16" => {
                println!("--- 快速点单流程（支持选配/特调） ---");
                println!("提示: 门店信息请先从【查看配送地址】或【查询附近门店】中获取");
                let store = read_line("门店 storeCode: ")?;
                let be = read_line_opt("门店 beCode（到店/得来速回车跳过）: ");
                let address = read_line_opt("地址 ID（到店/得来速回车跳过）: ");
                let ot = read_line("订单类型 (1=到店, 2=外送): ")?;
                let order_type: i32 = ot.parse().unwrap_or(1);
                let bt = read_line("beType (1=到店, 2=麦乐送, 5=得来速, 6=团餐，默认1）: ")?;
                let be_type: i32 = if bt.is_empty() {
                    1
                } else {
                    bt.parse().unwrap_or(1)
                };
                let rd = read_line_opt("预约时间（回车跳过）: ");

                let mut basket: Vec<Value> = Vec::new();
                loop {
                    let code = read_line("商品 code（直接回车结束添加）: ")?;
                    if code.is_empty() {
                        break;
                    }
                    let detail = match fetch_meal_detail(
                        client,
                        &code,
                        &store,
                        be.as_deref(),
                        order_type,
                        be_type,
                        rd.as_deref(),
                    )
                    .await
                    {
                        Ok(detail) => detail,
                        Err(e) => {
                            println!("❌ 查询餐品失败: {}", e);
                            continue;
                        }
                    };
                    match interactively_select(&detail, 1) {
                        Ok(item) => {
                            basket.push(item);
                            println!("✅ 已加入购物篮，共 {} 件", basket.len());
                        }
                        Err(e) => println!("❌ 选餐失败: {}", e),
                    }
                }

                if basket.is_empty() {
                    println!("购物篮为空，返回菜单");
                    continue;
                }
                println!("📦 购物篮:");
                for item in &basket {
                    println!("  {}", serde_json::to_string(item)?);
                }
                let items = serde_json::to_string(&basket)?;
                println!("\n正在计算价格...");
                if let Err(e) = run_calculate_price(
                    client,
                    &store,
                    be.as_deref(),
                    order_type,
                    be_type,
                    rd.as_deref(),
                    None,
                    None,
                    &items,
                )
                .await
                {
                    println!("❌ 计价失败: {}", e);
                    continue;
                }
                let confirm = read_line("确认下单? (y/n): ")?;
                if confirm.eq_ignore_ascii_case("y") {
                    println!("\n正在创建订单...");
                    let take_way = if order_type == 1 || be_type == 5 {
                        read_line_opt(
                            "取餐方式编码 takeWayCode（从calculate-price结果获取，回车跳过）: ",
                        )
                    } else {
                        None
                    };
                    let coupon_id = read_line_opt("优惠券 ID（回车跳过）: ");
                    let coupon_code = read_line_opt("优惠券编码（回车跳过）: ");
                    let gm_code = if be_type == 6 {
                        read_line_opt("助餐服务 code（团餐必填，回车跳过）: ")
                    } else {
                        None
                    };
                    if let Err(e) = run_order_create(
                        client,
                        &store,
                        be.as_deref(),
                        address.as_deref(),
                        &items,
                        order_type,
                        be_type,
                        take_way.as_deref(),
                        rd.as_deref(),
                        coupon_id.as_deref(),
                        coupon_code.as_deref(),
                        gm_code.as_deref(),
                    )
                    .await
                    {
                        println!("❌ 下单失败: {}", e);
                    }
                } else {
                    println!("已取消下单");
                }
            }
            "17" => {
                let id = read_line("订单号 orderId: ")?;
                if let Err(e) = run_order_query(client, &id).await {
                    println!("❌ 查询失败: {}", e);
                }
            }
            "0" | "q" | "quit" | "exit" => {
                println!("再见，祝您用餐愉快! 🍔");
                break;
            }
            _ => println!("无效选项，请重新输入"),
        }
    }
    Ok(())
}

fn resolve_token(cli_token: Option<String>, cfg: &Config) -> Result<String> {
    cli_token
        .or_else(|| std::env::var("MCD_MCP_TOKEN").ok())
        .or_else(|| cfg.token.clone())
        .context("错误: 需要提供 MCP Token\n方式1: mcd-cli login --token xxx\n方式2: 环境变量 MCD_MCP_TOKEN=xxx\n方式3: 命令行参数 --token xxx")
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load().context("加载配置文件失败")?;

    match cli.command {
        Some(Commands::Login { token, url }) => {
            let mut cfg = Config::load()?;
            cfg.set_token(token);
            if let Some(u) = url {
                cfg.set_url(u);
            }
            cfg.save()?;
            println!("✅ Token 已保存到: {}", Config::config_path()?.display());
            return Ok(());
        }
        Some(Commands::Config) => {
            let cfg = Config::load()?;
            println!("配置文件路径: {}", Config::config_path()?.display());
            println!("token: {}", cfg.token.as_deref().unwrap_or("(未设置)"));
            println!(
                "url:   {}",
                cfg.url.as_deref().unwrap_or("(默认 https://mcp.mcd.cn)")
            );
            return Ok(());
        }
        _ => {}
    }

    let token = resolve_token(cli.token, &cfg)?;
    let url = cfg.url.unwrap_or(cli.url);
    let client = McpClient::with_url(&url, token)?;

    match cli.command {
        Some(Commands::Init) => run_init(&client).await?,
        Some(Commands::Calendar) => run_calendar(&client).await?,
        Some(Commands::Time) => run_time(&client).await?,
        Some(Commands::Account) => run_account(&client).await?,
        Some(Commands::Nearby {
            be_type,
            search_type,
            city,
            keyword,
        }) => {
            run_nearby(
                &client,
                be_type,
                search_type,
                city.as_deref(),
                keyword.as_deref(),
            )
            .await?
        }
        Some(Commands::Address { action }) => match action {
            AddressCommands::List { be_type } => run_address_list(&client, be_type).await?,
            AddressCommands::Add { be_type } => run_address_add(&client, be_type).await?,
        },
        Some(Commands::DeliveryStores {
            address_id,
            be_type,
        }) => run_delivery_stores(&client, &address_id, be_type).await?,
        Some(Commands::Catering {
            store,
            be,
            reservation_date,
        }) => run_catering(&client, &store, &be, reservation_date.as_deref()).await?,
        Some(Commands::Nutrition) => run_nutrition(&client).await?,
        Some(Commands::Menu {
            store,
            be,
            order_type,
            be_type,
            reservation_date,
        }) => {
            run_menu(
                &client,
                &store,
                be.as_deref(),
                order_type,
                be_type,
                reservation_date.as_deref(),
            )
            .await?
        }
        Some(Commands::Detail {
            code,
            store,
            be,
            order_type,
            be_type,
            reservation_date,
            mods,
        }) => {
            fmt::set_show_mods(mods);
            run_detail(
                &client,
                &code,
                &store,
                be.as_deref(),
                order_type,
                be_type,
                reservation_date.as_deref(),
            )
            .await?
        }
        Some(Commands::Select {
            code,
            store,
            be,
            order_type,
            be_type,
            reservation_date,
            qty,
            pick,
            json,
        }) => {
            run_select(
                &client,
                &code,
                &store,
                be.as_deref(),
                order_type,
                be_type,
                reservation_date.as_deref(),
                qty,
                pick.as_deref(),
                json,
            )
            .await?
        }
        Some(Commands::Coupon { action }) => match action {
            CouponCommands::Store {
                store,
                be,
                order_type,
                be_type,
                reservation_date,
            } => {
                run_coupon_store(
                    &client,
                    &store,
                    &be,
                    order_type,
                    be_type,
                    reservation_date.as_deref(),
                )
                .await?
            }
            CouponCommands::My => run_coupon_my(&client).await?,
            CouponCommands::Available => run_coupon_available(&client).await?,
            CouponCommands::Receive => run_coupon_receive(&client).await?,
        },
        Some(Commands::Mall { action }) => match action {
            MallCommands::Products { cat_rule_ids } => {
                run_mall_products(&client, cat_rule_ids.as_deref()).await?
            }
            MallCommands::Detail { spu_id } => run_mall_detail(&client, spu_id).await?,
            MallCommands::Exchange { sku_id, count } => {
                run_mall_exchange(&client, sku_id, count).await?
            }
            MallCommands::Physical {
                sku_id,
                count,
                address_id,
                spu_category,
            } => run_mall_physical(&client, sku_id, count, &address_id, &spu_category).await?,
            MallCommands::Orders { last_id, size } => {
                run_mall_orders(&client, last_id, size).await?
            }
            MallCommands::OrderDetail { order_id } => {
                run_mall_order_detail(&client, &order_id).await?
            }
        },
        Some(Commands::Price {
            store,
            be,
            order_type,
            be_type,
            reservation_date,
            coupon_id,
            coupon_code,
            items,
        }) => {
            run_calculate_price(
                &client,
                &store,
                be.as_deref(),
                order_type,
                be_type,
                reservation_date.as_deref(),
                coupon_id.as_deref(),
                coupon_code.as_deref(),
                &items,
            )
            .await?
        }
        Some(Commands::Order { action }) => match action {
            OrderCommands::Create {
                store,
                be,
                address,
                items,
                order_type,
                be_type,
                take_way,
                reservation_date,
                coupon_id,
                coupon_code,
                gm_service_code,
            } => {
                run_order_create(
                    &client,
                    &store,
                    be.as_deref(),
                    address.as_deref(),
                    &items,
                    order_type,
                    be_type,
                    take_way.as_deref(),
                    reservation_date.as_deref(),
                    coupon_id.as_deref(),
                    coupon_code.as_deref(),
                    gm_service_code.as_deref(),
                )
                .await?
            }
            OrderCommands::Query { id } => run_order_query(&client, &id).await?,
        },
        Some(Commands::Interactive) | None => {
            println!("🍟 麦当劳 MCP CLI v{}", env!("CARGO_PKG_VERSION"));
            println!("正在进入交互模式...\n");
            interactive_mode(&client).await?;
        }
        _ => unreachable!(),
    }

    Ok(())
}
