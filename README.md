# mcd-cli

麦当劳 MCP CLI —— 基于麦当劳中国官方 MCP Server 的命令行点餐工具。

## 功能

- 🔍 查询附近门店、浏览菜单、查看餐品详情
- 🍟 **选餐定制**：随心配/套餐轮次选配 + 特调（去冰、加酱等），一键生成下单 JSON
- 🥗 餐品营养信息查询（热量、蛋白质、脂肪等）
- 🎫 查看/领取优惠券
- 🛒 计算价格、创建订单、查询订单状态
- 🎁 积分商城兑换（虚拟券 + 实物商品）
- 📅 活动日历、账户积分查询
- 🚗 支持到店取餐、麦乐送外送、得来速车道取餐、企业团餐
- ⏰ 支持预约下单

## 安装

```bash
git clone <repo>
cd mcd-cli
cargo build --release
```

编译完成后，二进制文件位于 `target/release/mcd-cli`。

## 配置

### 1. 获取 MCP Token

访问 [https://open.mcd.cn/mcp](https://open.mcd.cn/mcp)，登录后进入控制台激活 MCP Token。

### 2. 保存 Token

```bash
# 保存到配置文件（~/.config/mcd-cli/config.toml）
./mcd-cli login --token <YOUR_MCP_TOKEN>

# 查看配置
./mcd-cli config
```

Token 优先级：命令行参数 `--token` > 环境变量 `MCD_MCP_TOKEN` > 配置文件。

## 使用

### 交互模式（推荐）

```bash
./mcd-cli
```

进入交互式菜单，按提示操作即可。

### 命令行模式

```bash
# 测试连接
./mcd-cli init

# 查询附近门店（到店自取）
./mcd-cli nearby --city "南京市" --keyword "南京审计大学" --be-type 1 --search-type 2

# 查询得来速门店
./mcd-cli nearby --city "南京市" --keyword "麦当劳" --be-type 5 --search-type 2

# 外送可配送门店查询
./mcd-cli delivery-stores --address-id <ADDRESS_ID> --be-type 2

# 团餐助餐服务查询
./mcd-cli catering --store <STORE_CODE> --be <BE_CODE>

# 餐品营养信息
./mcd-cli nutrition

# 浏览菜单（到店取餐）
./mcd-cli menu --store 1990366 --order-type 1 --be-type 1

# 浏览菜单（外送）
./mcd-cli menu --store 1960282 --be 196028202 --order-type 2 --be-type 2

# 浏览菜单（得来速）
./mcd-cli menu --store 1990366 --order-type 1 --be-type 5

# 浏览菜单（预约）
./mcd-cli menu --store 1990366 --order-type 1 --be-type 1 --reservation-date "2026-05-25 12:00"

# 餐品详情
./mcd-cli detail 4820 --store 1990366 --order-type 1 --be-type 1

# 餐品详情（展开特调选项）
./mcd-cli detail 9900013304 --store 1990366 --order-type 1 --be-type 1 --mods

# 选餐定制：交互式选配（随心配 蓝区/粉区）并可特调，生成 items JSON
./mcd-cli select 9900013304 --store 1990366 --order-type 1 --be-type 1

# 选餐定制：非交互选配（轮次序号=商品code），只输出 items JSON（供管道使用）
./mcd-cli select 9900013304 --store 1990366 --order-type 1 --be-type 1 \
  --pick "1=1600,2=3050" --json

# 选餐结果直接送入价格计算
ITEMS=$(./mcd-cli select 9900013304 --store 1990366 --order-type 1 --be-type 1 \
  --pick "1=1450,2=3050" --json | tail -1)
./mcd-cli price --store 1990366 --order-type 1 --be-type 1 --items "$ITEMS"

# 计算价格（到店取餐）
./mcd-cli price --store 1990366 --order-type 1 --be-type 1 \
  --items '[{"productCode":"9900005462","quantity":1}]'

# 计算价格（使用优惠券）
./mcd-cli price --store 1990366 --order-type 1 --be-type 1 \
  --items '[{"productCode":"9900005462","quantity":1}]' \
  --coupon-id <COUPON_ID>

# 创建订单（到店取餐）
./mcd-cli order create --store 1990366 --order-type 1 --be-type 1 \
  --items '[{"productCode":"9900005462","quantity":1}]' \
  --take-way take-in-store

# 创建订单（外送）
./mcd-cli order create --store 1960282 --be 196028202 --address <ADDRESS_ID> --order-type 2 --be-type 2 \
  --items '[{"productCode":"903050","quantity":1}]'

# 创建订单（预约）
./mcd-cli order create --store 1990366 --order-type 1 --be-type 1 \
  --items '[{"productCode":"9900005462","quantity":1}]' \
  --take-way take-in-store \
  --reservation-date "2026-05-25 12:00"

# 创建订单（团餐）
./mcd-cli order create --store <STORE> --be <BE> --order-type 2 --be-type 6 \
  --items '[{"productCode":"xxx","quantity":1}]' \
  --gm-service-code <CODE>

# 查询订单
./mcd-cli order query <ORDER_ID>

# 查看优惠券
./mcd-cli coupon my
./mcd-cli coupon receive

# 积分商城
./mcd-cli mall products
./mcd-cli mall detail <SPU_ID>
./mcd-cli mall exchange --sku-id <SKU_ID> --count 1
./mcd-cli mall physical --sku-id <SKU_ID> --count 1 --address-id <ADDRESS_ID> --spu-category 2
./mcd-cli mall orders
./mcd-cli mall order-detail <ORDER_ID>
```

## 命令列表

| 命令 | 说明 |
|------|------|
| `init` | 测试 MCP 连接 |
| `login` | 保存 Token 到配置文件 |
| `config` | 查看当前配置 |
| `time` | 查看当前时间 |
| `calendar` | 查看活动日历 |
| `account` | 查看账户/积分 |
| `nearby` | 查询附近门店（到店/得来速） |
| `delivery-stores` | 外送/团餐可配送门店查询 |
| `catering` | 查询团餐助餐服务 |
| `nutrition` | 餐品营养信息列表 |
| `address list` | 查看配送地址 |
| `address add` | 新增配送地址 |
| `menu` | 浏览菜单 |
| `detail` | 餐品详情（`--mods` 展开特调） |
| `select` | 选餐定制：轮次选配+特调，生成 items JSON（交互/`--pick`） |
| `coupon store` | 门店可用优惠券 |
| `coupon my` | 我的优惠券 |
| `coupon available` | 可领优惠券列表 |
| `coupon receive` | 一键领券 |
| `mall products` | 积分兑换商品列表 |
| `mall detail` | 积分商品详情 |
| `mall exchange` | 积分兑换券下单（虚拟） |
| `mall physical` | 积分兑换实物下单 |
| `mall orders` | 麦麦商城订单查询 |
| `mall order-detail` | 麦麦商城订单详情查询 |
| `price` | 计算价格 |
| `order create` | 创建订单 |
| `order query` | 查询订单 |
| `interactive` | 交互式菜单 |

## 技术说明

- 协议：MCP Streamable HTTP (`https://mcp.mcd.cn`)
- 认证：Bearer Token
- MCP 版本：`2025-06-18`
- API 版本：`v1.0.4`

## 免责声明

本工具仅供学习交流使用，请遵守麦当劳相关服务条款。使用本工具产生的任何后果由使用者自行承担。
