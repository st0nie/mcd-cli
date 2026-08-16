use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MealDetail {
    pub code: String,
    pub name: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub image: Option<String>,
    #[serde(rename = "supportModify")]
    #[allow(dead_code)]
    pub support_modify: bool,
    #[serde(default)]
    pub modification: Option<Modification>,
    #[serde(default)]
    pub rounds: Vec<Round>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Round {
    pub id: i64,
    pub name: String,
    pub quantity: i64,
    #[serde(rename = "maxQuantity")]
    pub max_quantity: i64,
    #[serde(rename = "minQuantity")]
    pub min_quantity: i64,
    #[serde(default)]
    pub choices: Vec<Choice>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub code: String,
    pub name: String,
    pub quantity: i64,
    #[serde(rename = "maxQuantity")]
    #[allow(dead_code)]
    pub max_quantity: i64,
    #[serde(rename = "isDefault")]
    pub is_default: i64,
    #[serde(rename = "diffPrice", default)]
    pub diff_price: Option<String>,
    #[serde(rename = "supportModify")]
    pub support_modify: bool,
    #[serde(default)]
    pub modification: Option<Modification>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Modification {
    #[serde(default)]
    pub items: Vec<ModItem>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModItem {
    #[serde(rename = "maxValues")]
    pub max_values: i64,
    #[serde(rename = "minValues")]
    pub min_values: i64,
    #[serde(default)]
    pub values: Vec<ModValue>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModValue {
    pub code: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub price: Option<i64>,
    pub name: String,
    #[serde(rename = "maxQuantity")]
    #[allow(dead_code)]
    pub max_quantity: i64,
    #[serde(rename = "minQuantity")]
    #[allow(dead_code)]
    pub min_quantity: i64,
    #[serde(rename = "selectedQuantity")]
    pub selected_quantity: i64,
    #[serde(rename = "selectedKey")]
    pub selected_key: String,
    #[serde(rename = "unselectedKey", default)]
    pub unselected_key: Option<String>,
}

pub fn parse_detail(data: &Value) -> Result<MealDetail> {
    serde_json::from_value(data.clone()).context("解析餐品详情失败")
}

pub struct ModSelection {
    pub values: Vec<Value>,
}

pub fn build_item_value(
    detail: &MealDetail,
    qty: u32,
    picks: &[Vec<usize>],
    mods: &[Vec<Option<ModSelection>>],
    product_mods: Option<ModSelection>,
) -> Value {
    let mut item = json!({"productCode": detail.code, "quantity": qty});

    if detail.rounds.is_empty() {
        if let Some(selection) = product_mods.filter(|selection| !selection.values.is_empty()) {
            item["modification"] = json!({"values": selection.values});
        }
    } else if !picks.is_empty() {
        let round_list: Vec<Value> = detail
            .rounds
            .iter()
            .enumerate()
            .filter_map(|(round_index, round)| {
                let round_picks = picks.get(round_index)?;
                if round_picks.is_empty() {
                    return None;
                }
                let combo_item_list: Vec<Value> = round_picks
                    .iter()
                    .enumerate()
                    .filter_map(|(pick_index, &pick)| {
                        let choice = round.choices.get(pick)?;
                        let mut combo = json!({"code": choice.code, "quantity": 1});
                        if let Some(Some(selection)) = mods
                            .get(round_index)
                            .and_then(|round_mods| round_mods.get(pick_index))
                            && !selection.values.is_empty()
                        {
                            combo["modification"] = json!({"values": selection.values});
                        }
                        Some(combo)
                    })
                    .collect();
                if combo_item_list.is_empty() {
                    return None;
                }
                Some(json!({
                    "round": round.id.to_string(),
                    "comboItemList": combo_item_list
                }))
            })
            .collect();
        item["roundList"] = Value::Array(round_list);
    }

    item
}

pub struct Ui<'a> {
    pub reader: &'a mut dyn BufRead,
    pub writer: &'a mut dyn Write,
}

impl Ui<'_> {
    pub fn ask(&mut self, prompt: &str) -> Result<String> {
        write!(self.writer, "{prompt}").context("输出提示失败")?;
        self.writer.flush().context("刷新输出失败")?;
        let mut line = String::new();
        let read = self.reader.read_line(&mut line).context("读取输入失败")?;
        if read == 0 {
            bail!("输入已结束");
        }
        Ok(line.trim().to_string())
    }

    pub fn ask_bool(&mut self, prompt: &str, default: bool) -> Result<bool> {
        loop {
            let answer = self.ask(prompt)?;
            if answer.is_empty() {
                return Ok(default);
            }
            match answer.to_ascii_lowercase().as_str() {
                "y" | "yes" | "是" => return Ok(true),
                "n" | "no" | "否" => return Ok(false),
                _ => writeln!(self.writer, "请输入 y 或 n").context("输出提示失败")?,
            }
        }
    }

    pub fn ask_idx(&mut self, prompt: &str, default: usize) -> Result<usize> {
        let answer = self.ask(prompt)?;
        if answer.is_empty() {
            return Ok(default);
        }
        answer.parse::<usize>().context("请输入有效编号")
    }
}

fn default_choice(round: &Round) -> usize {
    round
        .choices
        .iter()
        .position(|choice| choice.quantity > 0 || choice.is_default == 1)
        .unwrap_or(0)
}

fn choice_line(choice: &Choice, index: usize) -> String {
    let price = choice.diff_price.as_deref().unwrap_or("");
    let modify = if choice.support_modify {
        "【可特调】"
    } else {
        ""
    };
    let default = if choice.is_default == 1 || choice.quantity > 0 {
        "[默认]"
    } else {
        ""
    };
    format!(
        "  {}. {} {} {} {}",
        index + 1,
        choice.name,
        price,
        modify,
        default
    )
}

pub fn pick_round_choices(ui: &mut Ui<'_>, round: &Round, round_no: usize) -> Result<Vec<usize>> {
    writeln!(
        ui.writer,
        "轮次{}【{}】 (选{}~{}项, 当前已选{}):",
        round_no, round.name, round.min_quantity, round.max_quantity, round.quantity
    )
    .context("输出轮次失败")?;
    for (index, choice) in round.choices.iter().enumerate() {
        writeln!(ui.writer, "{}", choice_line(choice, index)).context("输出选项失败")?;
    }

    let default = default_choice(round) + 1;
    if round.min_quantity == 1 && round.max_quantity == 1 {
        loop {
            let answer = ui.ask(&format!("请选择 [默认{}]: ", default))?;
            let number = if answer.is_empty() {
                default
            } else {
                answer.parse::<usize>().context("请输入有效编号")?
            };
            if (1..=round.choices.len()).contains(&number) {
                return Ok(vec![number - 1]);
            }
            writeln!(ui.writer, "编号无效，请选择 1~{}", round.choices.len())
                .context("输出提示失败")?;
        }
    }

    loop {
        let answer = ui.ask(&format!("请选择编号（逗号分隔，默认{}）: ", default))?;
        let text = if answer.is_empty() {
            default.to_string()
        } else {
            answer
        };
        let mut picks = Vec::new();
        let mut valid = true;
        for part in text.split(',') {
            let number = match part.trim().parse::<usize>() {
                Ok(number) => number,
                Err(_) => {
                    valid = false;
                    break;
                }
            };
            if !(1..=round.choices.len()).contains(&number) || picks.contains(&(number - 1)) {
                valid = false;
                break;
            }
            picks.push(number - 1);
        }
        let count = picks.len() as i64;
        if valid && count >= round.min_quantity && count <= round.max_quantity {
            return Ok(picks);
        }
        writeln!(
            ui.writer,
            "选择数量无效，请输入{}~{}个不同编号",
            round.min_quantity, round.max_quantity
        )
        .context("输出提示失败")?;
    }
}

fn mod_value_json(value: &ModValue, key: &str, quantity: i64) -> Value {
    json!({"code": value.code, "key": key, "quantity": quantity})
}

fn render_mod_group(ui: &mut Ui<'_>, item: &ModItem) -> Result<Vec<Value>> {
    if item.values.is_empty() {
        return Ok(Vec::new());
    }
    let all_toggle = item
        .values
        .iter()
        .all(|value| value.unselected_key.is_some());
    if item.min_values == 1 && item.max_values == 1 && item.values.len() == 1 {
        let value = &item.values[0];
        return Ok(vec![mod_value_json(value, &value.selected_key, 1)]);
    }

    if all_toggle {
        let mut selected: Vec<bool> = item
            .values
            .iter()
            .map(|value| value.selected_quantity > 0)
            .collect();
        loop {
            for (index, value) in item.values.iter().enumerate() {
                let mark = if selected[index] { "[x]" } else { "[ ]" };
                writeln!(ui.writer, "  {}. {} {}", index + 1, mark, value.name)
                    .context("输出特调选项失败")?;
            }
            let answer = ui.ask("输入编号切换，回车完成: ")?;
            if answer.is_empty() {
                let selected_count = selected.iter().filter(|value| **value).count() as i64;
                if selected_count < item.min_values || selected_count > item.max_values {
                    writeln!(
                        ui.writer,
                        "当前已选{}项，请选择{}~{}项后回车",
                        selected_count, item.min_values, item.max_values
                    )
                    .context("输出提示失败")?;
                    continue;
                }
                let values = item
                    .values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        if selected[index] {
                            mod_value_json(value, &value.selected_key, 1)
                        } else {
                            mod_value_json(value, value.unselected_key.as_deref().unwrap_or(""), 0)
                        }
                    })
                    .collect();
                return Ok(values);
            }
            let mut valid = true;
            for part in answer.split(',') {
                let number = match part.trim().parse::<usize>() {
                    Ok(number) => number,
                    Err(_) => {
                        valid = false;
                        break;
                    }
                };
                if !(1..=selected.len()).contains(&number) {
                    valid = false;
                    break;
                }
                selected[number - 1] = !selected[number - 1];
            }
            if !valid {
                writeln!(ui.writer, "编号无效，请重新输入").context("输出提示失败")?;
            }
        }
    }

    let current = item
        .values
        .iter()
        .position(|value| value.selected_quantity > 0)
        .unwrap_or(0);
    for (index, value) in item.values.iter().enumerate() {
        let mark = if index == current { "[当前]" } else { "" };
        writeln!(ui.writer, "  {}. {} {}", index + 1, mark, value.name)
            .context("输出特调选项失败")?;
    }
    let selected = loop {
        let number = ui.ask_idx(&format!("请选择 [默认{}]: ", current + 1), current + 1)?;
        if (1..=item.values.len()).contains(&number) {
            break number - 1;
        }
        writeln!(ui.writer, "编号无效，请重新输入").context("输出提示失败")?;
    };
    let value = &item.values[selected];
    Ok(vec![mod_value_json(value, &value.selected_key, 1)])
}

fn pick_mod_groups(ui: &mut Ui<'_>, modification: &Modification) -> Result<ModSelection> {
    let mut values = Vec::new();
    for item in &modification.items {
        values.extend(render_mod_group(ui, item)?);
    }
    Ok(ModSelection { values })
}

pub fn pick_mods(ui: &mut Ui<'_>, choice: &Choice) -> Result<Option<ModSelection>> {
    let Some(modification) = choice.modification.as_ref() else {
        return Ok(None);
    };
    if modification.items.is_empty() {
        return Ok(None);
    }
    if !ui.ask_bool(&format!("特调 {}? (y/N) ", choice.name), false)? {
        return Ok(None);
    }
    Ok(Some(pick_mod_groups(ui, modification)?))
}

pub fn pick_product_mods(ui: &mut Ui<'_>, detail: &MealDetail) -> Result<Option<ModSelection>> {
    let Some(modification) = detail.modification.as_ref() else {
        return Ok(None);
    };
    if modification.items.is_empty() {
        return Ok(None);
    }
    if !ui.ask_bool("该商品可特调，是否特调? (y/N) ", false)? {
        return Ok(None);
    }
    Ok(Some(pick_mod_groups(ui, modification)?))
}

fn selected_mod_names(choice: &Choice, selection: &ModSelection) -> Vec<String> {
    choice
        .modification
        .as_ref()
        .map(|modification| {
            modification
                .items
                .iter()
                .flat_map(|item| &item.values)
                .filter(|value| {
                    selection.values.iter().any(|selected| {
                        selected.get("code").and_then(Value::as_str) == Some(value.code.as_str())
                            && selected.get("key").and_then(Value::as_str)
                                == Some(value.selected_key.as_str())
                    })
                })
                .map(|value| value.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

pub fn render_selection_summary(
    detail: &MealDetail,
    picks: &[Vec<usize>],
    mods: &[Vec<Option<ModSelection>>],
) -> String {
    let mut lines = Vec::new();
    for (round_index, round) in detail.rounds.iter().enumerate() {
        let Some(round_picks) = picks.get(round_index).filter(|picks| !picks.is_empty()) else {
            continue;
        };
        let names: Vec<String> = round_picks
            .iter()
            .enumerate()
            .filter_map(|(pick_index, &pick)| {
                let choice = round.choices.get(pick)?;
                let mut line = choice.name.clone();
                if let Some(Some(selection)) = mods
                    .get(round_index)
                    .and_then(|round_mods| round_mods.get(pick_index))
                {
                    let mod_names = selected_mod_names(choice, selection);
                    if !mod_names.is_empty() {
                        line.push_str(&format!(" (特调: {})", mod_names.join(", ")));
                    }
                }
                Some(line)
            })
            .collect();
        if !names.is_empty() {
            lines.push(format!("  {}: {}", round.name, names.join(" + ")));
        }
    }
    if lines.is_empty() {
        lines.push(format!("  {}", detail.name));
    }
    lines.join("\n")
}

pub fn parse_pick_spec(detail: &MealDetail, spec: &str) -> Result<Vec<Vec<usize>>> {
    let mut picks: Vec<Vec<usize>> = vec![Vec::new(); detail.rounds.len()];
    let mut seen = vec![false; detail.rounds.len()];
    let mut assignments: Vec<(String, String)> = Vec::new();
    for part in spec
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        if let Some((round_text, code)) = part.split_once('=') {
            assignments.push((round_text.trim().to_string(), code.trim().to_string()));
        } else if let Some((_, codes)) = assignments.last_mut() {
            if !codes.is_empty() {
                codes.push('|');
            }
            codes.push_str(part);
        } else {
            bail!("选择规格无效: {part}");
        }
    }
    for (round_text, codes_text) in assignments {
        let round_number = round_text
            .parse::<usize>()
            .with_context(|| format!("轮次编号无效: {round_text}"))?;
        let round_index = round_number
            .checked_sub(1)
            .filter(|&index| index < detail.rounds.len())
            .with_context(|| format!("轮次编号无效: {round_text}"))?;
        let round = &detail.rounds[round_index];
        let codes: Vec<&str> = codes_text
            .split('|')
            .map(str::trim)
            .filter(|code| !code.is_empty())
            .collect();
        if codes.is_empty() {
            bail!(
                "轮次{}【{}】未选择商品，有效编码: {}",
                round_number,
                round.name,
                valid_codes(round)
            );
        }
        if (codes.len() as i64) < round.min_quantity || (codes.len() as i64) > round.max_quantity {
            bail!(
                "轮次{}【{}】选择数量无效，有效编码: {}",
                round_number,
                round.name,
                valid_codes(round)
            );
        }
        if seen[round_index] {
            bail!(
                "轮次{}【{}】重复指定，有效编码: {}",
                round_number,
                round.name,
                valid_codes(round)
            );
        }
        let mut found = Vec::new();
        for code in codes {
            let Some(index) = round.choices.iter().position(|choice| choice.code == code) else {
                bail!(
                    "轮次{}【{}】无效编码 {}，有效编码: {}",
                    round_number,
                    round.name,
                    code,
                    valid_codes(round)
                );
            };
            if found.contains(&index) {
                bail!(
                    "轮次{}【{}】重复编码 {}，有效编码: {}",
                    round_number,
                    round.name,
                    code,
                    valid_codes(round)
                );
            }
            found.push(index);
        }
        picks[round_index] = found;
        seen[round_index] = true;
    }
    if let Some((index, round)) = detail
        .rounds
        .iter()
        .enumerate()
        .find(|(index, _)| !seen[*index])
    {
        bail!(
            "轮次{}【{}】缺少选择，有效编码: {}",
            index + 1,
            round.name,
            valid_codes(round)
        );
    }
    Ok(picks)
}

fn valid_codes(round: &Round) -> String {
    round
        .choices
        .iter()
        .map(|choice| choice.code.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
