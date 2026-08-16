use qrcode::{QrCode, render::unicode};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use tabled::{builder::Builder, settings::Style};
use unicode_width::UnicodeWidthStr;

static SHOW_MODS: AtomicBool = AtomicBool::new(false);

pub fn set_show_mods(v: bool) {
    SHOW_MODS.store(v, Ordering::Relaxed);
}

fn pad(s: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - w))
    }
}

fn render_qr(url: &str) -> Option<String> {
    QrCode::new(url.as_bytes())
        .ok()
        .map(|code| code.render::<unicode::Dense1x2>().quiet_zone(true).build())
}

fn extract_order_id(url: &str) -> Option<&str> {
    url.split("orderId=")
        .nth(1)
        .map(|s| s.split('&').next().unwrap_or(s))
}

fn money_cents(cents: i64) -> String {
    format!("¥{:.2}", cents as f64 / 100.0)
}

fn money_str(s: &str) -> String {
    if let Ok(v) = s.parse::<f64>() {
        format!("¥{:.2}", v)
    } else {
        s.to_string()
    }
}

fn get_data(val: &Value) -> &Value {
    val.get("data").unwrap_or(&Value::Null)
}

pub fn pretty_print(val: &Value) {
    if let Some(text) = val.as_str() {
        // MCP returns markdown+JSON text, try extract JSON
        if let Some(idx) = text.find("{\"success\":")
            && let Ok(json) = serde_json::from_str::<Value>(&text[idx..])
        {
            pretty_print_json(&json);
            return;
        }
        println!("{}", text);
        return;
    }
    pretty_print_json(val);
}

fn pretty_print_json(val: &Value) {
    if val.get("data").is_some() {
        if val["data"].get("categories").is_some() && val["data"].get("meals").is_some() {
            println!("{}", format_menu(val));
            return;
        }
        if val["data"].get("addresses").is_some() {
            println!("{}", format_addresses(val));
            return;
        }
        if val["data"].get("goods").is_some() && val["data"].get("orderStatusTitle").is_some() {
            println!("{}", format_mall_order_detail(val));
            return;
        }
        if val["data"].get("orderStatus").is_some() || val["data"].get("orderId").is_some() {
            println!("{}", format_order(val));
            return;
        }
        if val["data"].get("productList").is_some() {
            println!("{}", format_price(val));
            return;
        }
        if val["data"].is_array()
            && val["data"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false)
        {
            if val["data"][0].get("storeCode").is_some() {
                println!("{}", format_nearby(val));
                return;
            }
            if val["data"][0].get("spuId").is_some() || val["data"][0].get("spuName").is_some() {
                println!("{}", format_mall_products(val));
                return;
            }
            if val["data"][0].get("list").is_some() {
                println!("{}", format_mall_orders(val));
                return;
            }
            // store coupons array
            println!("{}", format_coupons(val));
            return;
        }
        if val["data"].get("coupons").is_some() {
            println!("{}", format_coupons(val));
            return;
        }
        if val["data"].get("availablePoint").is_some() {
            println!("{}", format_account(val));
            return;
        }
        if val["data"].get("timestamp").is_some() {
            println!("{}", format_time(val));
            return;
        }
        if val["data"].get("detail").is_some() && val["data"].get("spuName").is_some() {
            println!("{}", format_mall_detail(val));
            return;
        }
        if val["data"].get("exchangeResult").is_some() || val["data"].get("orderNo").is_some() {
            println!("{}", format_mall_order(val));
            return;
        }
        if val["data"].get("dailyList").is_some() {
            println!("{}", format_calendar(val));
            return;
        }
        if val["data"].get("mealAssistanceItems").is_some() {
            println!("{}", format_catering(val));
            return;
        }
        if val["data"].get("rounds").is_some() {
            println!(
                "{}",
                format_meal_detail(val, SHOW_MODS.load(Ordering::Relaxed))
            );
            return;
        }
        if val["data"].get("goods").is_some() && val["data"].get("orderStatusTitle").is_some() {
            println!("{}", format_mall_order_detail(val));
            return;
        }
        if val["data"].is_string() {
            let s = val["data"].as_str().unwrap_or("");
            if s.contains("energyKj") || s.contains("protein") {
                println!("{}", format_nutrition(s));
                return;
            }
        }
    }
    if val.get("success").is_some() && val.get("code").is_some() && val.get("message").is_some() {
        let success = val["success"].as_bool().unwrap_or(false);
        let code = val["code"].as_i64().unwrap_or(0);
        let msg = val["message"].as_str().unwrap_or("");
        if success {
            println!("✅ 成功 (code: {}): {}", code, msg);
        } else {
            println!("❌ 失败 (code: {}): {}", code, msg);
        }
        return;
    }
    // fallback: pretty JSON
    println!("{}", serde_json::to_string_pretty(val).unwrap_or_default());
}

fn format_menu(val: &Value) -> String {
    let mut out = String::new();
    out.push_str("\n🍔 菜单\n");

    let data = get_data(val);
    let meals_map = data
        .get("meals")
        .and_then(|m| m.as_object())
        .unwrap_or(&serde_json::Map::new())
        .clone();
    let categories = data
        .get("categories")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    let mut builder = Builder::default();
    builder.push_record(["分类", "餐品编码", "餐品名称", "价格", "标签"]);

    for cat in categories {
        let cat_name_raw = cat
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
                    .unwrap_or("未知");
                let price = info
                    .and_then(|i| i.get("currentPrice"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("-");
                let mut tags = meal
                    .get("tags")
                    .and_then(|t| t.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                let with_order = info
                    .and_then(|i| i.get("discountType"))
                    .and_then(|v| v.as_str())
                    .map(|v| v.contains("随单购"))
                    .unwrap_or(false)
                    || info
                        .and_then(|i| i.get("canWithOrder"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                if with_order {
                    if tags.is_empty() {
                        tags = "随单购".to_string();
                    } else {
                        tags.push_str(", 随单购");
                    }
                }
                builder.push_record([
                    cat_name_raw.as_str(),
                    code,
                    name,
                    &format!("¥{}", price),
                    &tags,
                ]);
            }
        }
    }

    let table = builder.build().with(Style::modern()).to_string();
    out.push_str(&table);
    out.push('\n');
    out
}

fn format_addresses(val: &Value) -> String {
    let mut out = String::new();
    out.push_str("\n📍 配送地址\n");

    let data = get_data(val);
    let addresses = data
        .get("addresses")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();

    if addresses.is_empty() {
        out.push_str("  (暂无配送地址)\n\n");
        return out;
    }

    let mut builder = Builder::default();
    builder.push_record(["联系人", "电话", "地址", "门店", "storeCode", "beCode"]);
    for addr in addresses {
        let contact = addr
            .get("contactName")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let phone = addr.get("phone").and_then(|v| v.as_str()).unwrap_or("-");
        let full_addr = addr
            .get("fullAddress")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let store = addr
            .get("storeName")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let sc = addr
            .get("storeCode")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let bc = addr.get("beCode").and_then(|v| v.as_str()).unwrap_or("-");
        builder.push_record([contact, phone, full_addr, store, sc, bc]);
    }
    let table = builder.build().with(Style::modern()).to_string();
    out.push_str(&table);
    out.push('\n');
    out
}

fn format_order(val: &Value) -> String {
    let mut out = String::new();
    let data = get_data(val);

    let order_id = data.get("orderId").and_then(|v| v.as_str()).unwrap_or("-");
    let status = data
        .get("orderStatus")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let store = data
        .get("storeName")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let create_time = data
        .get("createTime")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let total = data
        .get("realTotalAmount")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let product_price = data
        .get("productPrice")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let delivery = data
        .get("realDeliveryPrice")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let discount = data
        .get("totalDiscountAmount")
        .and_then(|v| v.as_str())
        .unwrap_or("0");

    out.push_str("\n📋 订单详情\n");
    out.push_str("═".repeat(50).as_str());
    out.push('\n');
    out.push_str(&format!("  订单号:   {}\n", order_id));
    out.push_str(&format!("  状态:     {}\n", status));
    out.push_str(&format!("  门店:     {}\n", store));
    out.push_str(&format!("  下单时间: {}\n", create_time));
    out.push('\n');
    out.push_str("  💰 费用明细\n");
    out.push_str(&format!("     商品价格: {}\n", money_str(product_price)));
    out.push_str(&format!("     配送费:   {}\n", money_str(delivery)));
    out.push_str(&format!("     优惠:     -{}\n", money_str(discount)));
    out.push_str("     ─────────────────\n");
    out.push_str(&format!("     实付:     {}\n", money_str(total)));
    out.push('\n');

    if let Some(products) = data.get("orderProductList").and_then(|v| v.as_array()) {
        out.push_str("  🍔 商品清单\n");
        for p in products {
            let name = p.get("productName").and_then(|v| v.as_str()).unwrap_or("-");
            let qty = p.get("quantity").and_then(|v| v.as_i64()).unwrap_or(0);
            let price = p.get("price").and_then(|v| v.as_str()).unwrap_or("-");
            out.push_str(&format!(
                "     {} x{}  {}\n",
                pad(name, 20),
                qty,
                money_str(price)
            ));
        }
        out.push('\n');
    }

    if let Some(delivery_info) = data.get("deliveryInfo") {
        let addr = delivery_info
            .get("deliveryAddress")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let detail = delivery_info
            .get("addressDetail")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let nickname = delivery_info
            .get("customerNickname")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let phone = delivery_info
            .get("mobilePhone")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let dtype = delivery_info
            .get("deliveryType")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        out.push_str("  🚚 配送信息\n");
        out.push_str(&format!("     收件人: {}  {}\n", nickname, phone));
        out.push_str(&format!("     地址:   {} {}\n", addr, detail));
        out.push_str(&format!("     方式:   {}\n", dtype));
        out.push('\n');
    }

    if let Some(url) = data.get("payH5Url").and_then(|v| v.as_str()) {
        out.push_str(&format!("  💳 支付链接: {}\n", url));
        let qr_url = extract_order_id(url)
            .map(|id| format!("https://m.mcd.cn/mcp/jumpToApp?orderId={}", id))
            .unwrap_or_else(|| url.to_string());
        out.push_str("  📱 扫码支付 (麦当劳APP):\n");
        if let Some(qr) = render_qr(&qr_url) {
            for line in qr.lines() {
                out.push_str(&format!("    {}\n", line));
            }
        } else {
            out.push_str("    (无法生成二维码)\n");
        }
        out.push('\n');
    }

    out
}

fn format_price(val: &Value) -> String {
    let mut out = String::new();
    let data = get_data(val);

    let _original = data
        .get("originalPrice")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let price = data.get("price").and_then(|v| v.as_i64()).unwrap_or(0);
    let discount = data.get("discount").and_then(|v| v.as_i64()).unwrap_or(0);
    let delivery = data
        .get("deliveryPrice")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let packing = data
        .get("packingPrice")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    out.push_str("\n💰 价格计算\n");
    out.push_str("═".repeat(50).as_str());
    out.push('\n');

    if let Some(products) = data.get("productList").and_then(|v| v.as_array()) {
        for p in products {
            let name = p.get("productName").and_then(|v| v.as_str()).unwrap_or("-");
            let qty = p.get("quantity").and_then(|v| v.as_i64()).unwrap_or(0);
            let sub = p.get("subtotal").and_then(|v| v.as_i64()).unwrap_or(0);
            out.push_str(&format!(
                "  {} x{}  {}\n",
                pad(name, 22),
                qty,
                money_cents(sub)
            ));
        }
    }

    out.push('\n');
    out.push_str(&format!(
        "  商品小计:     {}\n",
        money_cents(price - delivery - packing)
    ));
    if delivery > 0 {
        out.push_str(&format!("  配送费:       {}\n", money_cents(delivery)));
    }
    if packing > 0 {
        out.push_str(&format!("  打包费:       {}\n", money_cents(packing)));
    }
    if discount > 0 {
        out.push_str(&format!("  优惠:        -{}\n", money_cents(discount)));
    }
    out.push_str("  ─────────────────────\n");
    out.push_str(&format!("  实付合计:     {}\n", money_cents(price)));

    if let Some(take_ways) = data.get("takeWayList").and_then(|v| v.as_array()) {
        out.push('\n');
        out.push_str("  📦 取餐方式:\n");
        for tw in take_ways {
            let code = tw.get("code").and_then(|v| v.as_str()).unwrap_or("-");
            let title = tw.get("title").and_then(|v| v.as_str()).unwrap_or("-");
            let sub = tw.get("subtitle").and_then(|v| v.as_str()).unwrap_or("");
            out.push_str(&format!("     {} — {} (code: {})\n", title, sub, code));
        }
    }

    out.push('\n');
    out
}

fn format_coupons(val: &Value) -> String {
    let mut out = String::new();
    let data = get_data(val);

    out.push_str("\n🎫 优惠券\n");

    // query-my-coupons: data.coupons
    if let Some(coupons) = data.get("coupons").and_then(|v| v.as_array()) {
        if coupons.is_empty() {
            out.push_str("  (暂无优惠券)\n\n");
            return out;
        }
        let mut builder = Builder::default();
        builder.push_record(["券名", "券码", "有效期", "状态"]);
        for c in coupons {
            let title = c.get("title").and_then(|v| v.as_str()).unwrap_or("-");
            let code = c.get("code").and_then(|v| v.as_str()).unwrap_or("-");
            let time = c
                .get("instructions")
                .and_then(|i| i.get("availableTime"))
                .and_then(|a| a.as_array())
                .and_then(|arr| arr.first())
                .and_then(|t| t.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let status = c
                .get("instructions")
                .and_then(|i| i.get("labels"))
                .and_then(|a| a.as_array())
                .and_then(|arr| arr.first())
                .and_then(|t| t.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            builder.push_record([title, code, time, status]);
        }
        let table = builder.build().with(Style::modern()).to_string();
        out.push_str(&table);
        out.push('\n');
        return out;
    }

    // query-store-coupons: data is array
    let coupons = data.as_array().cloned().unwrap_or_default();
    if coupons.is_empty() {
        out.push_str("  (暂无可用优惠券)\n\n");
        return out;
    }
    let mut builder = Builder::default();
    builder.push_record(["券名", "券ID", "券码", "有效期", "适用商品"]);
    for c in coupons {
        let title = c.get("title").and_then(|v| v.as_str()).unwrap_or("-");
        let coupon_id = c.get("couponId").and_then(|v| v.as_str()).unwrap_or("-");
        let code = c.get("couponCode").and_then(|v| v.as_str()).unwrap_or("-");
        let time = c
            .get("tradeDateTime")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let products = c
            .get("products")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| p.get("productName").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        builder.push_record([title, coupon_id, code, time, &products]);
    }
    let table = builder.build().with(Style::modern()).to_string();
    out.push_str(&table);
    out.push('\n');
    out
}

fn format_account(val: &Value) -> String {
    let mut out = String::new();
    let data = get_data(val);

    let available = data
        .get("availablePoint")
        .and_then(|v| v.as_str())
        .unwrap_or("0");
    let accum = data
        .get("accumulativePoint")
        .and_then(|v| v.as_str())
        .unwrap_or("0");
    let used = data
        .get("usedPoint")
        .and_then(|v| v.as_str())
        .unwrap_or("0");
    let expired = data
        .get("expiredPoint")
        .and_then(|v| v.as_str())
        .unwrap_or("0");
    let expire_this = data
        .get("currentMouthExpirePoint")
        .and_then(|v| v.as_str())
        .unwrap_or("0");
    let frozen = data
        .get("frozenPoint")
        .and_then(|v| v.as_str())
        .unwrap_or("0");
    let currency = data
        .get("currency")
        .and_then(|v| v.as_str())
        .unwrap_or("积分");

    out.push_str("\n👤 我的账户\n");
    out.push_str("═".repeat(40).as_str());
    out.push('\n');
    out.push_str(&format!("  可用积分:    {} {}\n", available, currency));
    out.push_str(&format!("  累计积分:    {}\n", accum));
    out.push_str(&format!("  已使用:      {}\n", used));
    out.push_str(&format!("  已过期:      {}\n", expired));
    out.push_str(&format!("  本月将过期:  {}\n", expire_this));
    out.push_str(&format!("  冻结中:      {}\n", frozen));
    out.push('\n');
    out
}

fn format_time(val: &Value) -> String {
    let data = get_data(val);
    let formatted = data
        .get("formatted")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let _date = data.get("date").and_then(|v| v.as_str()).unwrap_or("-");
    let week = data
        .get("dayOfWeek")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let tz = data.get("timezone").and_then(|v| v.as_str()).unwrap_or("-");

    format!("\n🕐 当前时间: {} {} ({})\n\n", formatted, week, tz)
}

fn format_nearby(val: &Value) -> String {
    let mut out = String::new();
    let data = get_data(val);
    let stores = data.as_array().cloned().unwrap_or_default();

    out.push_str("\n📍 附近门店\n");

    if stores.is_empty() {
        out.push_str("  (未找到门店)\n\n");
        return out;
    }

    let mut builder = Builder::default();
    builder.push_record(["门店编码", "门店名称", "距离", "地址"]);
    for s in stores {
        let sc = s.get("storeCode").and_then(|v| v.as_str()).unwrap_or("-");
        let name = s.get("storeName").and_then(|v| v.as_str()).unwrap_or("-");
        let dist = s.get("distance").and_then(|v| v.as_i64()).unwrap_or(0);
        let addr = s.get("address").and_then(|v| v.as_str()).unwrap_or("-");
        builder.push_record([sc, name, &format!("{}m", dist), addr]);
    }
    let table = builder.build().with(Style::modern()).to_string();
    out.push_str(&table);
    out.push('\n');
    out
}

fn format_mall_products(val: &Value) -> String {
    let mut out = String::new();
    let data = get_data(val);
    let products = data.as_array().cloned().unwrap_or_default();

    out.push_str("\n🎁 积分商城\n");

    if products.is_empty() {
        out.push_str("  (暂无商品)\n\n");
        return out;
    }

    let mut builder = Builder::default();
    builder.push_record(["商品名称", "所需积分", "spuId", "skuId", "卖点"]);
    for p in products {
        let name = p.get("spuName").and_then(|v| v.as_str()).unwrap_or("-");
        let point = p.get("point").and_then(|v| v.as_str()).unwrap_or("-");
        let spu_id = p
            .get("spuId")
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            .to_string();
        let sku_id = p
            .get("skuId")
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            .to_string();
        let selling = p.get("selling").and_then(|v| v.as_str()).unwrap_or("");
        builder.push_record([name, point, &spu_id, &sku_id, selling]);
    }
    let table = builder.build().with(Style::modern()).to_string();
    out.push_str(&table);
    out.push('\n');
    out
}

fn format_mall_detail(val: &Value) -> String {
    let mut out = String::new();
    let data = get_data(val);

    let name = data.get("spuName").and_then(|v| v.as_str()).unwrap_or("-");
    let point = data.get("points").and_then(|v| v.as_str()).unwrap_or("-");
    let sku_id = data.get("skuId").and_then(|v| v.as_i64()).unwrap_or(0);
    let note = data.get("note").and_then(|v| v.as_str()).unwrap_or("");
    let detail = data.get("detail").and_then(|v| v.as_str()).unwrap_or("");

    out.push_str("\n🎁 商品详情\n");
    out.push_str("═".repeat(50).as_str());
    out.push('\n');
    out.push_str(&format!("  名称: {}\n", name));
    out.push_str(&format!("  所需积分: {}\n", point));
    out.push_str(&format!("  skuId: {}\n", sku_id));
    if !note.is_empty() {
        out.push_str(&format!("  须知: {}\n", note));
    }
    if !detail.is_empty() {
        out.push_str(&format!("  详情: {}\n", detail));
    }
    out.push('\n');
    out
}

fn format_mall_order(val: &Value) -> String {
    let mut out = String::new();
    let data = get_data(val);

    if let Some(order_no) = data.get("orderNo").and_then(|v| v.as_str()) {
        out.push_str(&format!("\n✅ 兑换成功！订单号: {}\n\n", order_no));
    } else if let Some(result) = data.get("exchangeResult").and_then(|v| v.as_str()) {
        out.push_str(&format!("\n✅ 兑换结果: {}\n\n", result));
    } else {
        out.push_str("\n✅ 操作成功\n\n");
    }
    out
}

fn format_nutrition(data_str: &str) -> String {
    let mut out = String::new();
    out.push_str("\n🥗 餐品营养信息\n");

    // Parse header: [count]{field1,field2,...}:
    let Some(header_start) = data_str.find('{') else {
        out.push_str("  (无法解析数据)\n\n");
        return out;
    };
    let Some(header_end) = data_str.find("}:") else {
        out.push_str("  (无法解析数据)\n\n");
        return out;
    };
    let header = &data_str[header_start + 1..header_end];
    let fields: Vec<&str> = header.split(',').collect();

    if fields.is_empty() {
        out.push_str("  (无数据)\n\n");
        return out;
    }

    let mut builder = Builder::default();
    builder.push_record(fields.clone());

    // Parse data lines after the header
    let data_part = &data_str[header_end + 2..];
    for line in data_part.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let values: Vec<&str> = line.split(',').collect();
        builder.push_record(values);
    }

    let table = builder.build().with(Style::modern()).to_string();
    out.push_str(&table);
    out.push('\n');
    out
}

fn format_calendar(val: &Value) -> String {
    let mut out = String::new();
    let data = get_data(val);

    out.push_str("\n📅 活动日历\n");
    out.push_str("═".repeat(50).as_str());
    out.push('\n');

    let daily_list = data
        .get("dailyList")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if daily_list.is_empty() {
        out.push_str("  (暂无活动)\n\n");
        return out;
    }

    for day in daily_list {
        let date_text = day.get("dateText").and_then(|v| v.as_str()).unwrap_or("-");
        out.push_str(&format!("\n  📆 {}\n", date_text));

        if let Some(events) = day.get("events").and_then(|v| v.as_array()) {
            for event in events {
                let title = event
                    .get("activityTitle")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        event
                            .get("articleDto")
                            .and_then(|a| a.get("title"))
                            .and_then(|v| v.as_str())
                    })
                    .unwrap_or("-");
                let content = event
                    .get("activitySubTitle")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        event
                            .get("articleDto")
                            .and_then(|a| a.get("content"))
                            .and_then(|v| v.as_str())
                    })
                    .unwrap_or("");
                out.push_str(&format!("     • {}\n", title));
                if !content.is_empty() {
                    for line in content.lines() {
                        let line = line.trim();
                        if !line.is_empty() {
                            out.push_str(&format!("       {}\n", line));
                        }
                    }
                }
            }
        }
    }
    out.push('\n');
    out
}

fn format_catering(val: &Value) -> String {
    let mut out = String::new();
    let data = get_data(val);

    out.push_str("\n🍱 助餐服务\n");
    out.push_str("═".repeat(50).as_str());
    out.push('\n');

    let items = data
        .get("mealAssistanceItems")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        out.push_str("  (当前门店无可选助餐服务)\n\n");
        return out;
    }

    let mut builder = Builder::default();
    builder.push_record(["服务编码", "服务名称", "服务内容", "状态"]);
    for item in items {
        let code = item
            .get("gmServiceCode")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let name = item
            .get("gmServiceName")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let services = item
            .get("serviceItems")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default();
        let enable = item
            .get("enable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let status = if enable { "可选" } else { "不可选" };
        builder.push_record([code, name, &services, status]);
    }
    let table = builder.build().with(Style::modern()).to_string();
    out.push_str(&table);
    out.push('\n');

    if let Some(promos) = data.get("promotions").and_then(|v| v.as_array()) {
        out.push_str("\n  🎁 阶梯优惠:\n");
        for p in promos {
            if let Some(text) = p.as_str() {
                out.push_str(&format!("     • {}\n", text));
            }
        }
    }
    out.push('\n');
    out
}

fn append_modification_groups(out: &mut String, modification: &Value) {
    if let Some(items) = modification.get("items").and_then(|v| v.as_array()) {
        for item in items {
            let min_values = item.get("minValues").and_then(|v| v.as_i64()).unwrap_or(0);
            let max_values = item.get("maxValues").and_then(|v| v.as_i64()).unwrap_or(0);
            let values = item.get("values").and_then(|v| v.as_array());
            let value_names = values
                .map(|values| {
                    let mut names = values
                        .iter()
                        .take(5)
                        .map(|value| {
                            let selected = value
                                .get("selectedQuantity")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0)
                                > 0;
                            let name = value.get("name").and_then(|v| v.as_str()).unwrap_or("-");
                            format!("{}{}", if selected { "✓" } else { "✗" }, name)
                        })
                        .collect::<Vec<_>>();
                    if values.len() > 5 {
                        names.push("...".to_string());
                    }
                    names.join(" / ")
                })
                .unwrap_or_default();
            out.push_str(&format!(
                "        特调组: {} (选{}~{})\n",
                value_names, min_values, max_values
            ));
        }
    }
}

fn format_meal_detail(val: &Value, show_mods: bool) -> String {
    let mut out = String::new();
    let data = get_data(val);
    let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("-");
    let code = data.get("code").and_then(|v| v.as_str()).unwrap_or("-");
    let support_modify = data
        .get("supportModify")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    out.push_str("\n🔍 餐品详情\n");
    out.push_str("═".repeat(50).as_str());
    out.push('\n');
    out.push_str(&format!(
        "  名称: {}{}\n",
        name,
        if support_modify {
            " (该商品可特调)"
        } else {
            ""
        }
    ));
    out.push_str(&format!("  编码: {}\n", code));

    let rounds = data
        .get("rounds")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if !rounds.is_empty() {
        out.push_str("\n  🔄 选配:\n");
        for (round_index, round) in rounds.iter().enumerate() {
            let round_name = round.get("name").and_then(|v| v.as_str()).unwrap_or("选配");
            let min_quantity = round
                .get("minQuantity")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let max_quantity = round
                .get("maxQuantity")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let quantity = round.get("quantity").and_then(|v| v.as_i64()).unwrap_or(0);
            let selected_mark = if quantity >= min_quantity { "*" } else { "" };
            out.push_str(&format!(
                "  轮次{}【{}】 (选{}~{}项, 已选{}){}\n",
                round_index + 1,
                round_name,
                min_quantity,
                max_quantity,
                quantity,
                selected_mark
            ));
            if let Some(choices) = round.get("choices").and_then(|v| v.as_array()) {
                for (choice_index, choice) in choices.iter().enumerate() {
                    let choice_name = choice.get("name").and_then(|v| v.as_str()).unwrap_or("-");
                    let diff_price = choice
                        .get("diffPrice")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let diff_price = if diff_price == "+ ¥0" {
                        "¥0"
                    } else {
                        diff_price
                    };
                    let support_modify = choice
                        .get("supportModify")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let is_default = choice
                        .get("isDefault")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0)
                        == 1
                        || choice.get("quantity").and_then(|v| v.as_i64()).unwrap_or(0) > 0;
                    let mut marks = Vec::new();
                    if !diff_price.is_empty() {
                        marks.push(diff_price);
                    }
                    if support_modify {
                        marks.push("【可特调】");
                    }
                    if is_default {
                        marks.push("[默认]");
                    }
                    let suffix = if marks.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", marks.join(" "))
                    };
                    out.push_str(&format!(
                        "    {}. {}{}\n",
                        choice_index + 1,
                        choice_name,
                        suffix
                    ));
                    if show_mods && let Some(modification) = choice.get("modification") {
                        append_modification_groups(&mut out, modification);
                    }
                }
            }
        }
    } else if show_mods
        && let Some(modification) = data.get("modification")
        && modification
            .get("items")
            .and_then(|v| v.as_array())
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    {
        out.push_str("\n  可特调项:\n");
        append_modification_groups(&mut out, modification);
    }

    out.push_str("\n  当前默认搭配:\n");
    let default_combos = rounds
        .iter()
        .filter_map(|round| {
            let round_name = round.get("name").and_then(|v| v.as_str()).unwrap_or("选配");
            let choices = round.get("choices").and_then(|v| v.as_array())?;
            let names = choices
                .iter()
                .filter(|choice| {
                    choice
                        .get("isDefault")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0)
                        == 1
                        || choice.get("quantity").and_then(|v| v.as_i64()).unwrap_or(0) > 0
                })
                .filter_map(|choice| choice.get("name").and_then(|v| v.as_str()))
                .collect::<Vec<_>>();
            if names.is_empty() {
                None
            } else {
                Some(format!("{}: {}", round_name, names.join(" + ")))
            }
        })
        .collect::<Vec<_>>();
    out.push_str(&format!(
        "    {}\n",
        if default_combos.is_empty() {
            "-".to_string()
        } else {
            default_combos.join(" + ")
        }
    ));
    out.push_str("  提示: 用 select 命令可交互选配并生成下单 items JSON\n");
    out.push('\n');
    out
}

fn format_mall_orders(val: &Value) -> String {
    let mut out = String::new();
    let data = get_data(val);

    out.push_str("\n🛒 商城订单\n");

    let list_wrapper = data
        .as_array()
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or(Value::Null);
    let orders = list_wrapper
        .get("list")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if orders.is_empty() {
        out.push_str("  (暂无订单)\n\n");
        return out;
    }

    let mut builder = Builder::default();
    builder.push_record(["订单号", "状态", "商品", "数量"]);
    for order in orders {
        let oid = order.get("orderId").and_then(|v| v.as_str()).unwrap_or("-");
        let status = order
            .get("orderStatusTitle")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let total = order
            .get("totalCount")
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            .to_string();
        let goods = order
            .get("goods")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|g| g.get("spuName").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        builder.push_record([oid, status, &goods, &total]);
    }
    let table = builder.build().with(Style::modern()).to_string();
    out.push_str(&table);
    out.push('\n');

    let has_next = list_wrapper
        .get("hasNext")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if has_next {
        let last_id = list_wrapper
            .get("lastId")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        out.push_str(&format!(
            "\n  ➡️  还有更多订单，翻页请用 --last-id {}\n",
            last_id
        ));
    }
    out.push('\n');
    out
}

fn format_mall_order_detail(val: &Value) -> String {
    let mut out = String::new();
    let data = get_data(val);

    let oid = data.get("orderId").and_then(|v| v.as_str()).unwrap_or("-");
    let status = data
        .get("orderStatusTitle")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let sub_status = data
        .get("orderStatusSubTitle")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let pay = data
        .get("payChannelLabel")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let create_time = data
        .get("createTime")
        .and_then(|v| v.as_str())
        .unwrap_or("-");
    let phone = data
        .get("customerServicePhone")
        .and_then(|v| v.as_str())
        .unwrap_or("-");

    out.push_str("\n📋 商城订单详情\n");
    out.push_str("═".repeat(50).as_str());
    out.push('\n');
    out.push_str(&format!("  订单号:   {}\n", oid));
    out.push_str(&format!("  状态:     {}\n", status));
    if !sub_status.is_empty() {
        out.push_str(&format!("  备注:     {}\n", sub_status));
    }
    out.push_str(&format!("  支付方式: {}\n", pay));
    out.push_str(&format!("  下单时间: {}\n", create_time));
    out.push_str(&format!("  客服电话: {}\n", phone));

    let goods = data
        .get("goods")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if !goods.is_empty() {
        out.push_str("\n  🎁 商品:\n");
        for g in goods {
            let name = g.get("spuName").and_then(|v| v.as_str()).unwrap_or("-");
            let sku = g.get("skuName").and_then(|v| v.as_str()).unwrap_or("");
            let count = g.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
            let points = g.get("points").and_then(|v| v.as_str()).unwrap_or("0");
            if sku.is_empty() || sku == name {
                out.push_str(&format!("     • {} x{} ({}积分)\n", name, count, points));
            } else {
                out.push_str(&format!(
                    "     • {} ({}) x{} ({}积分)\n",
                    name, sku, count, points
                ));
            }
        }
    }

    if let Some(addr) = data.get("address") {
        let name = addr.get("name").and_then(|v| v.as_str()).unwrap_or("-");
        let mobile = addr.get("mobile").and_then(|v| v.as_str()).unwrap_or("-");
        let full = addr
            .get("fullAddress")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        out.push_str(&format!("\n  📍 收货地址: {} {} {}\n", name, mobile, full));
    }

    if let Some(logistics) = data.get("logistics") {
        let company = logistics
            .get("company")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let no = logistics
            .get("logisticsNo")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        out.push_str(&format!("\n  🚚 物流: {} {}\n", company, no));
    }

    out.push('\n');
    out
}
