# mcd-cli Complete Command Reference

## Store queries

### Nearby stores (dine-in / drive-thru)

```bash
mcd-cli nearby --city <CITY> --keyword <KEYWORD> --be-type <BE_TYPE> --search-type 2
```

| Parameter | Description |
|---|---|
| `--city` | City name, e.g. "南京市" |
| `--keyword` | Search keyword, e.g. "南京审计大学" or "麦当劳" |
| `--be-type` | `1` = dine-in, `5` = drive-thru |
| `--search-type` | `2` = keyword search |

### Delivery / catering stores

Requires a saved delivery address first.

```bash
mcd-cli address list --be-type 2
mcd-cli delivery-stores --address-id <ADDRESS_ID> --be-type <BE_TYPE>
```

| `--be-type` | Scene |
|---|---|
| `2` | McDelivery |
| `6` | Catering |

### Catering service info

```bash
mcd-cli catering --store <STORE_CODE> --be <BE_CODE>
```

Returns `gmServiceCode` which is required for catering orders.

## Menu & product detail

### Browse menu

```bash
mcd-cli menu --store <STORE_CODE> --order-type <ORDER_TYPE> --be-type <BE_TYPE>
```

With reservation:

```bash
mcd-cli menu --store <STORE_CODE> --order-type 1 --be-type 1 --reservation-date "2026-05-25 12:00"
```

| Parameter | Values |
|---|---|
| `--order-type` | `1` = dine-in/drive-thru, `2` = delivery/catering |
| `--be-type` | `1` = dine-in, `2` = McDelivery, `5` = drive-thru, `6` = catering |
| `--reservation-date` | Format `yyyy-MM-dd HH:mm` |

### Product detail

```bash
mcd-cli detail <PRODUCT_CODE> --store <STORE_CODE> --order-type 1 --be-type 1
```

Expand 特调 options:

```bash
mcd-cli detail <PRODUCT_CODE> --store <STORE_CODE> --order-type 1 --be-type 1 --mods
```

### Select / customize combo items (随心配 选餐)

`select` fetches `query-meal-detail`, walks every round (轮次) asking you to pick a choice, offers 特调 for choices that support it, and emits the exact `items[]` JSON payload for `price` / `order create`.

Interactive:

```bash
mcd-cli select <PRODUCT_CODE> --store <STORE_CODE> --order-type <ORDER_TYPE> --be-type <BE_TYPE>
```

Non-interactive (`--pick` = roundNumber=choiceCode, multiple codes comma-separated for multi-pick rounds; `--qty` = quantity):

```bash
mcd-cli select <PRODUCT_CODE> --store <STORE_CODE> --order-type 1 --be-type 1 \
  --pick "1=1600,2=3050" --qty 1 --json
```

Pipe into `price` directly:

```bash
ITEMS=$(mcd-cli select 9900013304 --store <STORE_CODE> --order-type 1 --be-type 1 \
  --pick "1=1600,2=3050" --json | tail -1)
mcd-cli price --store <STORE_CODE> --order-type 1 --be-type 1 --items "$ITEMS"
```

Example 随心配 codes (人气经典随心配 `9900013304`, 精选超值随心配 `9900013291`): round 1 蓝区 / round 2 粉区 both pick exactly 1 item (e.g. `1600` 麦香鱼, `3050` 可乐中杯). Product codes vary by store — verify with `mcd-cli menu`.

## Price calculation

```bash
mcd-cli price --store <STORE_CODE> --order-type 1 --be-type 1 \
  --items '[{"productCode":"9900005462","quantity":1}]'
```

With coupon:

```bash
mcd-cli price --store <STORE_CODE> --order-type 1 --be-type 1 \
  --items '[{"productCode":"9900005462","quantity":1}]' \
  --coupon-id <COUPON_ID>
```

The response includes `takeWayList[].code` which is needed for dine-in/drive-thru orders.

## Order management

### Create order (dine-in)

```bash
mcd-cli order create --store <STORE_CODE> --order-type 1 --be-type 1 \
  --items '[{"productCode":"9900005462","quantity":1}]' \
  --take-way <TAKE_WAY_CODE>
```

### Create order (drive-thru)

Same as dine-in but with `--be-type 5`.

### Create order (delivery)

```bash
mcd-cli order create --store <STORE_CODE> --be <BE_CODE> --address <ADDRESS_ID> \
  --order-type 2 --be-type 2 \
  --items '[{"productCode":"903050","quantity":1}]'
```

### Create order (catering)

```bash
mcd-cli order create --store <STORE_CODE> --be <BE_CODE> --order-type 2 --be-type 6 \
  --items '[{"productCode":"xxx","quantity":1}]' \
  --gm-service-code <GM_SERVICE_CODE>
```

### Create order (with reservation)

Append `--reservation-date "yyyy-MM-dd HH:mm"` to any order create command.

### Query order

```bash
mcd-cli order query <ORDER_ID>
```

Returns order status and `payH5Url` for payment.

### QR code output (`--qr` / `--qr-save`)

`order create` supports two QR output flags:

```bash
mcd-cli order create ... --qr       # print a unicode QR in the terminal
mcd-cli order create ... --qr-save  # write a PNG and print its path line
```

- `--qr-save`: writes the payment QR to `/tmp/mcd-qrcode/<orderId>.png` and prints one line
  `  已经保存到 /tmp/mcd-qrcode/<orderId>.png`. In chat scenarios (QQ/WeChat bot) send that PNG
  as an image message; on a desktop, open it with `open <path>` (macOS) or `xdg-open <path>` (Linux).
- `--qr`: prints a unicode QR directly in the terminal, for when the user is right at a terminal.
- Default (no flag): prints only the `payH5Url` text — no QR glyphs, no file.

## Coupons

```bash
mcd-cli coupon store     # store-available coupons
mcd-cli coupon my        # my coupons
mcd-cli coupon available # coupons available to claim
mcd-cli coupon receive   # claim all available coupons
```

## Loyalty points mall

```bash
mcd-cli mall products              # list products
mcd-cli mall detail <SPU_ID>        # product detail
mcd-cli mall exchange --sku-id <SKU_ID> --count 1                                        # virtual coupon
mcd-cli mall physical --sku-id <SKU_ID> --count 1 --address-id <ADDRESS_ID> --spu-category 2  # physical item
mcd-cli mall orders                # mall order history
mcd-cli mall order-detail <ORDER_ID>  # mall order detail
```

## Account & info

```bash
mcd-cli init       # test MCP connection
mcd-cli config     # view current config
mcd-cli time       # current time
mcd-cli calendar    # activity calendar
mcd-cli account    # account info & points
mcd-cli nutrition   # nutrition info for menu items
```

## Address management

```bash
mcd-cli address list   # list saved addresses
mcd-cli address add    # add new address
```

## Common product codes

| Product | Code |
|---|---|
| Quarter Pounder with Cheese meal | `9900005462` |
| McChicken meal | `9900005456` |
| Big Mac meal | `9900005466` |
| Large fries | `4820` |
| Medium cola | `903050` |

> Actual codes vary by store. Always verify with `mcd-cli menu`.

## TUI ordering

```bash
mcd-cli tui
```

Full-screen interactive ordering: store search → menu browse → 随心配 combo rounds + 特调 → basket → price → order create → QR code (saved to `/tmp/mcd-qrcode/<orderId>.png`). Keyboard: `↑↓`/`jk` navigate, `Enter` select, `Tab` switch input, `Space` toggle multi-select, `b` basket, `p` price, `q`/`Esc` back or quit.

## Notes

- Token rate limit: 600 requests/minute per token.
- Payment: `payH5Url` is a QR-code page; can also pay in McDonald's App under "My Orders".
- `price` supports `--coupon-id` and `--coupon-code` for discount calculation.
- `order create` also supports `--coupon-id` and `--coupon-code` to apply discounts.
