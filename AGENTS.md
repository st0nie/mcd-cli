# AGENTS.md

Rust CLI 包装麦当劳中国官方 MCP Server (`https://mcp.mcd.cn`, Streamable HTTP, Bearer token, protocol `2025-06-18`, 600 req/min/token)。无测试套件、无 CI。验证 = `cargo clippy` + `cargo build --release` + 真实 API 冒烟。

## 命令

- Build/lint: `cargo build --release`, `cargo clippy`（保持零警告）。`cargo fmt` 会重排整个文件（正常，已全仓格式化）。
- 无 `cargo test`。功能验证走真实 MCP API（需 token）。
- Token 来源优先级: `--token` > 环境变量 `MCD_MCP_TOKEN` > 配置文件。
- 配置文件路径 **`~/Library/Application Support/mcd-cli/config.toml`**（macOS `dirs::config_dir`，不是 `~/.config`！存错位置 token 静默读不到）。

## 结构

- `src/main.rs` — clap CLI + 交互模式（菜单编号，16=快速点单购物篮）。Handler 参数多，用 `#[allow(clippy::too_many_arguments)]`。
- `src/mcp.rs` — JSON-RPC 客户端。返回 `ToolResult { structured_content, content }`；响应 JSON 键为 camelCase。
- `src/meal.rs` — 选餐核心（纯 std sync，无 async/reqwest）：serde 模型 + `build_item_value` 生成 items JSON + `Ui` 交互选择。模型字段带 `#[allow(dead_code)]`（serde 解析需要但逻辑未读，勿删）。
- `src/fmt.rs` — 展示层。`pretty_print_json` 按 data 形状分派到各 formatter；`set_show_mods(bool)` 全局原子开关驱动 `detail --mods`。
- `skills/mcd-order/` — 给 Agent 用的命令手册（SKILL.md + references/command-reference.md），改命令必须同步改这里和 README。

## 选餐 payload（实测验证，勿猜）

`query-meal-detail` → `data.rounds[]`（选`minQuantity`~`maxQuantity`项，`quantity`=已选）→ `choices[]`：`isDefault`(1/0)、`diffPrice`("+ ¥0"/"- ¥1")、`supportModify`、`modification.items[].values[]`（`selectedKey`/`unselectedKey`）。

下单 items 格式：
```json
{"productCode":"9900013304","quantity":1,"roundList":[
  {"round":"1","comboItemList":[{"code":"1600","quantity":1,
    "modification":{"values":[{"code":"100136","key":"0-1","quantity":1}]}}]}]}
```

铁律：
- `round` 必须是**轮次 id 的字符串**（非下标、非数字）。
- 特调组所有 value 都有 `unselectedKey` 时（多选组）：**组内全部要传**，选中的 key=`selectedKey`，未选中的 key=`unselectedKey`，数量 0。
- 单数组（无 `unselectedKey`，如 标准/去冰/多冰/少冰）：只传选中的那项（key=其 `selectedKey`）。
- `min=1/max=1` 且仅 1 个 value 的组为必选，自动带入选中的 key。
- **随单购 withOrder 千万不要自动带**：服务器对不完整负载静默按原价结算（如 54元 而非 32元），用户会多付钱。只显示"随单购"标签提示。

## 实测定点（测试用）

- 用 `nearby` 找营业中的门店；门店打烊返回 `code 600057 门店可能已关闭`（如夜间店 1990366 会失败）。
- 已验证门店 `3450189`（成都双流长城路二段），随心配 `9900013304`：蓝区=轮次1、粉区=轮次2；麦香鱼(1600) 默认 14.9，麦香鸡(1450) -¥1 → 13.9；可乐(3050) 去冰 key=`0-0`（默认标准 key=`0-1`）。
- `calculate-price` `productList[].subtotal` 单位**分**（fmt `money_cents` /100 显示元）；`query-order` 等金额是字符串元。
- `select --json` 只向 stdout 输出紧凑 JSON 数组（banner 全部关闭），管道约定 `| tail -1`。
- 交互式 `select` 用 `Ui` 读 stdin，可 `printf '2\ny\n2\n\n2\n' | mcd-cli select ...` 管道模拟（每轮次1行输入，空行=默认）。