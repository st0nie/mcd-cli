---
name: mcd-order
description: >
  McDonald's China MCP ordering assistant. Browses menus, calculates prices,
  creates orders (dine-in, delivery, drive-thru, catering), queries order status,
  manages coupons, and exchanges loyalty points via the mcd-cli tool. Use when the
  user wants to order food, check menus, find nearby McDonald's stores, or manage
  their McDonald's account in China.
---

# McDonald's China MCP Ordering Assistant

## Prerequisites

The user must have an MCP Token from `https://open.mcd.cn/mcp`. Token configuration (pick one):

```bash
# Save to config file
mcd-cli login --token <TOKEN>

# Environment variable
export MCD_MCP_TOKEN=<TOKEN>

# Inline flag
mcd-cli --token <TOKEN> <command>
```

## Workflow

### 1. Find a store

**Dine-in / Drive-thru:**

```bash
mcd-cli nearby --city <CITY> --keyword <KEYWORD> --be-type <BE_TYPE> --search-type 2
```

| Scene | `--be-type` |
|---|---|
| Dine-in | `1` |
| Drive-thru | `5` |

**Delivery / Catering (address required first):**

```bash
mcd-cli address list --be-type 2
mcd-cli delivery-stores --address-id <ADDRESS_ID> --be-type <BE_TYPE>
```

| Scene | `--be-type` |
|---|---|
| McDelivery | `2` |
| Catering | `6` |

Save the `storeCode` for all subsequent steps.

### 2. Browse the menu

```bash
mcd-cli menu --store <STORE_CODE> --order-type <ORDER_TYPE> --be-type <BE_TYPE>
```

| Scene | `--order-type` | `--be-type` |
|---|---|---|
| Dine-in / Drive-thru | `1` | `1` or `5` |
| Delivery / Catering | `2` | `2` or `6` |

Reservation: append `--reservation-date "yyyy-MM-dd HH:mm"`.

Record the `productCode` of desired items.

### 3. Customize combos (随心配 / choose meal items)

`query-meal-detail` returns combo rounds (轮次选配) and 特调 (modification) options. Use `select`:

```bash
# Interactive: pick each round's item, optionally customize 特调 (去冰/加酱 etc.), outputs items JSON
mcd-cli select <PRODUCT_CODE> --store <STORE_CODE> --order-type <ORDER_TYPE> --be-type <BE_TYPE>

# Non-interactive: roundNumber=choiceCode, comma-separated rounds; --json prints ONLY the items JSON
mcd-cli select <PRODUCT_CODE> --store <STORE_CODE> --order-type 1 --be-type 1 \
  --pick "1=1600,2=3050" --json
```

Example — 随心配 (code `9900013304`, 蓝区=麦香鱼, 粉区=可乐中杯), then price it:

```bash
ITEMS=$(mcd-cli select 9900013304 --store <STORE_CODE> --order-type 1 --be-type 1 \
  --pick "1=1600,2=3050" --json | tail -1)
mcd-cli price --store <STORE_CODE> --order-type 1 --be-type 1 --items "$ITEMS"
```

The generated item includes `roundList` + `modification` (去冰/加料/去酱) entries that `price` and `order create` accept directly. Combo rounds where a choice supports 特调 are marked 【可特调】 — use `detail <CODE> --store <STORE> --order-type 1 --be-type 1 --mods` to preview all 特调 groups.

### 4. Calculate price

```bash
mcd-cli price --store <STORE_CODE> --order-type <ORDER_TYPE> --be-type <BE_TYPE> \
  --items '[{"productCode":"<CODE>","quantity":1}]'
```

For combos, pass `select`-generated items (with `roundList`/`modification`) instead of hand-written JSON:

With coupon: append `--coupon-id <COUPON_ID>` or `--coupon-code <CODE>`.

From the result, save `takeWayList[].code` (required for dine-in / drive-thru orders).

### 5. Create order

**Dine-in / Drive-thru:**

```bash
mcd-cli order create --store <STORE_CODE> --order-type 1 --be-type <BE_TYPE> \
  --items '[{"productCode":"<CODE>","quantity":1}]' \
  --take-way <TAKE_WAY_CODE>
```

**Delivery:**

```bash
mcd-cli order create --store <STORE_CODE> --be <BE_CODE> --address <ADDRESS_ID> \
  --order-type 2 --be-type 2 \
  --items '[{"productCode":"<CODE>","quantity":1}]'
```

**Catering:**

```bash
mcd-cli catering --store <STORE_CODE> --be <BE_CODE>   # get gmServiceCode first
mcd-cli order create --store <STORE_CODE> --be <BE_CODE> --order-type 2 --be-type 6 \
  --items '[{"productCode":"<CODE>","quantity":1}]' \
  --gm-service-code <GM_SERVICE_CODE>
```

**Reservation:** append `--reservation-date "yyyy-MM-dd HH:mm"` to any order create command.

### 6. Query & pay

```bash
mcd-cli order query <ORDER_ID>
```

The response contains `payH5Url` — a QR-code payment page. The user can also pay via the McDonald's App under "My Orders".

## Other features

```bash
# Coupons
mcd-cli coupon my              # my coupons
mcd-cli coupon available       # available to claim
mcd-cli coupon receive         # claim all

# Loyalty points mall
mcd-cli mall products          # list redeemable items
mcd-cli mall detail <SPU_ID>   # item detail
mcd-cli mall exchange --sku-id <SKU_ID> --count 1                          # virtual coupon
mcd-cli mall physical --sku-id <SKU_ID> --count 1 --address-id <ID> --spu-category 2  # physical item
mcd-cli mall orders            # mall order history
mcd-cli mall order-detail <ORDER_ID>

# Nutrition info
mcd-cli nutrition
```

## Gotchas

- Dine-in (`orderType=1, beType=1`) does **not** need `beCode` or `addressId`, but **requires** `takeWayCode` from the price result.
- Delivery (`orderType=2, beType=2`) **must** derive `storeCode`, `beCode`, and `addressId` from `delivery-stores` — never fabricate these values.
- Drive-thru (`beType=5`) behaves like dine-in: also needs `takeWayCode`.
- Catering (`beType=6`) requires `gmServiceCode` — run `mcd-cli catering` first.
- Always run `price` before `order create` to confirm the total and obtain `takeWayCode`.
- Each token is rate-limited to 600 requests/minute.

## Reference

See [references/command-reference.md](references/command-reference.md) for the complete command listing with all flags and examples.
