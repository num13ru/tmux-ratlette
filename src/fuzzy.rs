use crate::model::Item;

const ALIAS_EXACT_BOOST: usize = 100_000;

fn is_boundary(character: char) -> bool {
    character.is_whitespace() || matches!(character, '-' | '_' | '·' | '.' | '/' | ':')
}

fn exact_match_score(haystack: &[char], needle: &[char]) -> usize {
    if needle.is_empty() || needle.len() > haystack.len() {
        return 0;
    }

    let Some(index) = haystack
        .windows(needle.len())
        .position(|window| window == needle)
    else {
        return 0;
    };
    let at_boundary = index == 0 || is_boundary(haystack[index - 1]);
    (10_000 + usize::from(at_boundary) * 5_000).saturating_sub(index)
}

fn char_bonus(at_boundary: bool, consecutive: bool) -> usize {
    if at_boundary {
        50
    } else if consecutive {
        20
    } else {
        5
    }
}

fn subsequence_score(haystack: &[char], needle: &[char]) -> usize {
    let mut score = 0;
    let mut haystack_index = 0;
    let mut previous = None;

    for target in needle {
        while haystack_index < haystack.len() && haystack[haystack_index] != *target {
            haystack_index += 1;
        }
        if haystack_index >= haystack.len() {
            return 0;
        }

        let at_boundary = haystack_index == 0 || is_boundary(haystack[haystack_index - 1]);
        let consecutive = previous.is_some_and(|index| haystack_index == index + 1);
        score += char_bonus(at_boundary, consecutive);
        previous = Some(haystack_index);
        haystack_index += 1;
    }

    score.max(1)
}

fn fuzzy_score(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 1;
    }
    let haystack = haystack.to_lowercase().chars().collect::<Vec<_>>();
    let needle = needle.to_lowercase().chars().collect::<Vec<_>>();
    let exact = exact_match_score(&haystack, &needle);
    if exact > 0 {
        exact
    } else {
        subsequence_score(&haystack, &needle)
    }
}

pub fn multi_fuzzy_score(haystack: &str, parts: &[&str]) -> usize {
    let mut total: usize = 0;
    for part in parts {
        let score = fuzzy_score(haystack, part);
        if score == 0 {
            return 0;
        }
        total = total.saturating_add(score);
    }
    total
}

fn auto_alias(title: &str) -> Option<String> {
    let words = title
        .split_whitespace()
        .filter(|word| word.starts_with(|character: char| character.is_ascii_alphabetic()))
        .collect::<Vec<_>>();
    if words.len() < 2 {
        return None;
    }
    Some(
        words
            .iter()
            .filter_map(|word| word.chars().next())
            .collect::<String>()
            .to_ascii_lowercase(),
    )
}

fn item_haystack(item: &Item) -> String {
    let mut fields = vec![item.title.as_str()];
    fields.extend(item.description.as_deref());
    fields.extend(item.category.as_deref());
    fields.extend(item.shortcut.as_deref());
    fields.extend(item.aliases.iter().map(String::as_str));
    let auto_alias = auto_alias(&item.title);
    fields.extend(auto_alias.as_deref());
    fields.join(" ")
}

fn alias_exact_boost(item: &Item, parts: &[&str]) -> usize {
    let [query] = parts else {
        return 0;
    };
    let query = query.to_lowercase();
    if auto_alias(&item.title).is_some_and(|alias| alias == query) {
        return ALIAS_EXACT_BOOST;
    }
    if item
        .aliases
        .iter()
        .any(|alias| alias.to_lowercase() == query)
    {
        return ALIAS_EXACT_BOOST;
    }
    0
}

pub fn default_filter(items: &[Item], needle: &str) -> Vec<usize> {
    let parts = needle.split_whitespace().collect::<Vec<_>>();
    if parts.is_empty() {
        return (0..items.len()).collect();
    }

    let mut scored = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let score = multi_fuzzy_score(&item_haystack(item), &parts)
                .saturating_add(alias_exact_boost(item, &parts));
            (score > 0).then_some((index, score))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(left_index, left_score), (right_index, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left_index.cmp(right_index))
    });
    scored.into_iter().map(|(index, _)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Action;

    fn item(title: &str) -> Item {
        Item::new(title, Action::None)
    }

    fn titles<'a>(items: &'a [Item], indices: &[usize]) -> Vec<&'a str> {
        indices
            .iter()
            .map(|index| items[*index].title.as_str())
            .collect()
    }

    #[test]
    fn requires_every_query_part_to_match() {
        assert!(multi_fuzzy_score("split horizontal pane", &["split", "pane"]) > 0);
        assert_eq!(
            multi_fuzzy_score("split horizontal pane", &["split", "window"]),
            0
        );
    }

    #[test]
    fn matches_title_initials_through_auto_aliases() {
        let items = vec![
            item("Split Horizontal").category("Panes"),
            item("New Window").category("Windows"),
        ];

        assert_eq!(
            titles(&items, &default_filter(&items, "sh")),
            ["Split Horizontal"]
        );
    }

    #[test]
    fn matches_explicit_aliases() {
        let mut choose = item("Choose Session");
        choose.aliases.push("sessions".to_owned());
        let items = vec![choose, item("Detach")];

        assert_eq!(
            titles(&items, &default_filter(&items, "sessions"))[0],
            "Choose Session"
        );
    }

    #[test]
    fn exact_auto_aliases_outrank_category_substrings() {
        let items = vec![
            item("Detach").category("Sessions"),
            item("New Session").category("Sessions"),
            item("Next Session").category("Sessions"),
        ];

        let ranked = titles(&items, &default_filter(&items, "ns"));
        assert_eq!(&ranked[..2], ["New Session", "Next Session"]);
    }

    #[test]
    fn preserves_source_order_when_scores_tie() {
        let items = vec![item("New Session"), item("Next Session")];

        assert_eq!(
            titles(&items, &default_filter(&items, "ns")),
            ["New Session", "Next Session"]
        );
    }

    #[test]
    fn matches_unicode_without_slicing_invalid_utf8() {
        let items = vec![item("Résumé Pane"), item("Window")];

        assert_eq!(
            titles(&items, &default_filter(&items, "rés")),
            ["Résumé Pane"]
        );
    }

    #[test]
    fn very_long_haystacks_do_not_underflow_exact_match_scores() {
        let haystack = format!("{} match", "x".repeat(20_000));

        assert!(multi_fuzzy_score(&haystack, &["match"]) > 0);
    }

    #[test]
    fn matches_the_shared_typescript_parity_fixtures() {
        let fixture = include_str!("../tests/fixtures/fuzzy-parity.tsv");
        for line in fixture.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split('\t');
            let query = fields.next().unwrap();
            let item_specs = fields.next().unwrap();
            let expected = fields.next().unwrap_or_default();
            let items = item_specs
                .split('|')
                .map(|spec| {
                    let mut fields = spec.splitn(3, '~');
                    let title = fields.next().unwrap_or_default();
                    let category = fields.next().unwrap_or_default();
                    let aliases = fields.next().unwrap_or_default();
                    let mut item = item(title);
                    if !category.is_empty() {
                        item.category = Some(category.to_owned());
                    }
                    if !aliases.is_empty() {
                        item.aliases = aliases.split(',').map(str::to_owned).collect();
                    }
                    item
                })
                .collect::<Vec<_>>();
            let expected = if expected.is_empty() {
                Vec::new()
            } else {
                expected.split('|').collect::<Vec<_>>()
            };

            assert_eq!(
                titles(&items, &default_filter(&items, query)),
                expected,
                "query: {query:?}"
            );
        }
    }
}
