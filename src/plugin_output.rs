use serde_json::Value;

use crate::model::{Action, Item, ThemeColor};
use crate::user_config::{RawAction, RawItem};

pub(crate) fn parse(
    output: &[u8],
    template: Option<RawAction>,
    default_icon: Option<&str>,
    default_icon_color: Option<&str>,
) -> Result<Vec<Item>, String> {
    let output = String::from_utf8_lossy(output);
    let output = output.trim();
    if let Some(items) = parse_json_items(output) {
        return items;
    }
    parse_lines(output, template, default_icon, default_icon_color)
}

fn parse_json_items(output: &str) -> Option<Result<Vec<Item>, String>> {
    let Value::Array(values) = serde_json::from_str::<Value>(output).ok()? else {
        return None;
    };
    if !values.iter().all(Value::is_object) {
        return None;
    }
    Some(
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                serde_json::from_value::<RawItem>(value)
                    .map_err(|error| format!("invalid JSON item {index}: {error}"))?
                    .into_item(index)
            })
            .collect(),
    )
}

fn parse_lines(
    output: &str,
    template: Option<RawAction>,
    default_icon: Option<&str>,
    default_icon_color: Option<&str>,
) -> Result<Vec<Item>, String> {
    let template = template
        .ok_or_else(|| {
            "plain-text plugin output requires an action template containing {}".to_owned()
        })?
        .into_action("palette")?;
    let default_icon = nonempty(default_icon).map(str::to_owned);
    let default_icon_color = validated_color(nonempty(default_icon_color), "default iconColor")?;

    output
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            line_to_item(
                line,
                index,
                &template,
                default_icon.as_deref(),
                default_icon_color.as_deref(),
            )
        })
        .collect()
}

fn line_to_item(
    line: &str,
    index: usize,
    template: &Action,
    default_icon: Option<&str>,
    default_icon_color: Option<&str>,
) -> Result<Item, String> {
    let parts = line.split('\t').collect::<Vec<_>>();
    let (icon, icon_color, title) = match parts.as_slice() {
        [title] => (
            default_icon.map(str::to_owned),
            default_icon_color.map(str::to_owned),
            (*title).to_owned(),
        ),
        [icon, title] => (
            nonempty(Some(icon)).map(str::to_owned),
            default_icon_color.map(str::to_owned),
            (*title).to_owned(),
        ),
        [icon, color, title @ ..] => (
            nonempty(Some(icon)).map(str::to_owned),
            nonempty(Some(color)).map(str::to_owned),
            title.join("\t"),
        ),
        [] => unreachable!("split always returns at least one field"),
    };
    let title = sanitize_display(&title);
    if title.trim().is_empty() {
        return Err(format!("plain-text item {index} has an empty title"));
    }
    let icon_color = validated_color(
        icon_color.as_deref(),
        &format!("plain-text item {index} iconColor"),
    )?;
    let mut item = Item::new(&title, substitute(template, &title));
    item.icon = icon.map(|icon| sanitize_display(&icon));
    item.icon_color = icon_color;
    Ok(item)
}

fn substitute(template: &Action, title: &str) -> Action {
    match template {
        Action::Tmux(command) => Action::Tmux(command.replace("{}", title)),
        Action::Shell(command) => Action::Shell(command.replace("{}", title)),
        Action::Popup(action) => {
            let mut action = action.clone();
            action.command = action.command.replace("{}", title);
            Action::Popup(action)
        }
        Action::Palette(name) => Action::Palette(name.clone()),
        Action::ApplyTheme(slug) => Action::ApplyTheme(slug.clone()),
        Action::None => Action::None,
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

fn sanitize_display(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}

fn validated_color(value: Option<&str>, field: &str) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if ThemeColor::parse(value).is_none() {
        return Err(format!(
            "{field} has invalid color {value:?}; expected a hex, ANSI name, or transparent"
        ));
    }
    Ok(Some(value.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(json: &str) -> RawAction {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn parses_json_item_arrays_with_full_metadata_and_unknown_fields() {
        let items = parse(
            r##"[{"icon":"◆","iconColor":"#00ff00","title":"JSON item","description":"from JSON","shortcut":"M-j","category":"Tools","aliases":["ji"],"selectable":false,"action":{"shell":"echo json"},"future":true}]"##.as_bytes(),
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].icon.as_deref(), Some("◆"));
        assert_eq!(items[0].icon_color.as_deref(), Some("#00ff00"));
        assert_eq!(items[0].title, "JSON item");
        assert_eq!(items[0].description.as_deref(), Some("from JSON"));
        assert_eq!(items[0].shortcut.as_deref(), Some("M-j"));
        assert_eq!(items[0].category.as_deref(), Some("Tools"));
        assert_eq!(items[0].aliases, ["ji"]);
        assert!(!items[0].selectable);
        assert!(matches!(items[0].action, Action::Shell(ref value) if value == "echo json"));
    }

    #[test]
    fn parses_plain_lines_with_defaults_and_replaces_every_template_marker() {
        let items = parse(
            b"\nalpha\nbeta\n",
            Some(action(r#"{"shell":"run {} then {}"}"#)),
            Some("+"),
            Some("blue"),
        )
        .unwrap();

        assert_eq!(
            items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta"]
        );
        assert!(items.iter().all(|item| item.icon.as_deref() == Some("+")));
        assert!(
            items
                .iter()
                .all(|item| item.icon_color.as_deref() == Some("blue"))
        );
        assert!(
            matches!(items[0].action, Action::Shell(ref value) if value == "run alpha then alpha")
        );
    }

    #[test]
    fn popup_templates_preserve_overrides_while_replacing_the_command() {
        let items = parse(
            b"service-a",
            Some(action(
                r#"{"popup":"tail -f {}.log","width":"70%","height":"20","padX":2,"padY":1,"border":"rounded"}"#,
            )),
            None,
            None,
        )
        .unwrap();

        let Action::Popup(action) = &items[0].action else {
            panic!("expected popup action");
        };
        assert_eq!(action.command, "tail -f service-a.log");
        assert_eq!(action.width.as_deref(), Some("70%"));
        assert_eq!(action.height.as_deref(), Some("20"));
        assert_eq!(action.pad_x, Some(2));
        assert_eq!(action.pad_y, Some(1));
        assert_eq!(action.border.as_deref(), Some("rounded"));
    }

    #[test]
    fn parses_two_and_three_field_tab_rows_with_per_line_overrides() {
        let items = parse(
            b"A\tTwo fields\nB\t#ff0000\tThree\twith tab\n\t\tNo decoration",
            Some(action(r#"{"tmux":"display-message '{}'"}"#)),
            Some("default"),
            Some("green"),
        )
        .unwrap();

        assert_eq!(items[0].icon.as_deref(), Some("A"));
        assert_eq!(items[0].icon_color.as_deref(), Some("green"));
        assert_eq!(items[0].title, "Two fields");
        assert_eq!(items[1].icon.as_deref(), Some("B"));
        assert_eq!(items[1].icon_color.as_deref(), Some("#ff0000"));
        assert_eq!(items[1].title, "Three�with tab");
        assert_eq!(items[2].icon, None);
        assert_eq!(items[2].icon_color, None);
        assert_eq!(items[2].title, "No decoration");
        assert!(
            matches!(items[1].action, Action::Tmux(ref value) if value == "display-message 'Three�with tab'")
        );
    }

    #[test]
    fn converts_invalid_utf8_lossily_without_panicking() {
        let items = parse(b"broken-\xff", Some(action(r#"{"shell":":"}"#)), None, None).unwrap();

        assert_eq!(items[0].title, "broken-�");
    }

    #[test]
    fn non_object_json_values_fall_back_to_line_mode() {
        let items = parse(
            br#""quoted""#,
            Some(action(r#"{"palette":"find-pane"}"#)),
            None,
            None,
        )
        .unwrap();

        assert_eq!(items[0].title, r#""quoted""#);
        assert!(matches!(items[0].action, Action::Palette(ref value) if value == "find-pane"));
    }

    #[test]
    fn rejects_invalid_json_items_instead_of_creating_broken_actions() {
        let error = parse(br#"[{"title":"Missing action"}]"#, None, None, None).unwrap_err();
        let control = parse(
            br#"[{"title":"unsafe\u001btitle","action":{"shell":":"}}]"#,
            None,
            None,
            None,
        )
        .unwrap_err();

        assert!(error.contains("invalid JSON item 0"));
        assert!(error.contains("action"));
        assert!(control.contains("control character"));
    }

    #[test]
    fn line_mode_requires_a_valid_template_and_colors() {
        let missing = parse(b"item", None, None, None).unwrap_err();
        let bad_action = parse(
            b"item",
            Some(action(r#"{"shell":"x","tmux":"y"}"#)),
            None,
            None,
        )
        .unwrap_err();
        let bad_color = parse(
            b"item",
            Some(action(r#"{"shell":":"}"#)),
            None,
            Some("not-a-color"),
        )
        .unwrap_err();

        assert!(missing.contains("requires an action template"));
        assert!(bad_action.contains("palette action must contain exactly one"));
        assert!(bad_color.contains("default iconColor"));
    }

    #[test]
    fn an_empty_json_array_needs_no_template_but_empty_text_does() {
        assert!(parse(b"[]", None, None, None).unwrap().is_empty());
        assert!(
            parse(b"  \n", Some(action(r#"{"shell":":"}"#)), None, None)
                .unwrap()
                .is_empty()
        );
        assert!(parse(b"", None, None, None).is_err());
    }
}
