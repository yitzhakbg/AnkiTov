//! Card Renderer — Anki template subset → question/answer HTML (spec §4).
//!
//! Pure functions, no I/O: takes an Anki note (fields + template front/back)
//! and produces rendered question/answer HTML plus a fidelity-flag list.
//! The reviewer SPA consumes the JSON verbatim; the backend never executes
//! JavaScript from card content (XSS surface stays in the browser's own
//! sanitization — v1 renders Anki's own HTML subset as-is, matching what the
//! desktop client would show for the same note).
//!
//! ## Supported template subset (v1 coverage matrix, spec §4.2)
//!
//! | Feature | Support |
//! |---------|---------|
//! | `{{Field}}` replacement | yes |
//! | `{{cloze:Text}}` (c1..cn, active deletion by card ord) | yes |
//! | `{{type:Field}}` / `{{type:cloze:Text}}` | rendered as input placeholder |
//! | `{{#Field}}...{{/Field}}` conditional | yes |
//! | `{{^Field}}...{{/Field}}` negated conditional | yes |
//! | `hint:`, `text:`, `furigana:`, `kanji:`, `kana:` filters | hint/text yes; furigana family flagged |
//! | `[sound:file.mp3]` | → `<audio>` element |
//! | `<img src="...">`, media refs | passed through (media served by collection_store) |
//! | LaTeX `\(...\)` / `\[...\]` | flagged `mathjax` (SPA lazy-loads vendored MathJax) |
//! | `{{FrontSide}}`, `{{...}}` unknown filters | flagged `unsupported_filter`, rendered empty |
//!
//! RTL: the SPA sets direction per card via the `rtl` flag (note field heuristic).

use serde::Serialize;

/// Fidelity flags shipped per card (spec §4.3) — the SPA may adjust rendering
/// and the QA dashboard can query cards that degrade outside the subset.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FidelityFlags {
    /// Card content uses LaTeX — SPA should load MathJax for it.
    pub mathjax: bool,
    /// Template used a filter the subset does not implement.
    pub unsupported_filter: bool,
    /// Audio present (`[sound:...]`).
    pub audio: bool,
    /// Images present.
    pub images: bool,
    /// Rendered answer expects typed input (`{{type:...}}`).
    pub type_answer: bool,
    /// Heuristic: dominant script is RTL (Hebrew/Arabic) — SPA sets dir.
    pub rtl: bool,
}

impl FidelityFlags {
    fn none() -> Self {
        Self {
            mathjax: false,
            unsupported_filter: false,
            audio: false,
            images: false,
            type_answer: false,
            rtl: false,
        }
    }
}

/// One rendered card side.
#[derive(Debug, Clone, Serialize)]
pub struct RenderedSide {
    pub html: String,
}

/// A fully rendered card ready for the reviewer SPA.
#[derive(Debug, Clone, Serialize)]
pub struct RenderedCard {
    pub card_id: i64,
    pub note_id: i64,
    pub question: RenderedSide,
    pub answer: RenderedSide,
    pub flags: FidelityFlags,
    /// Deck name the card belongs to (for the reviewer header).
    pub deck_name: String,
}

/// Inputs gathered from the collection by `collection_store`.
pub struct RawCard {
    pub card_id: i64,
    pub note_id: i64,
    /// Note type ordinal of THIS card (cloze deletion index, 0-based).
    pub card_ord: i32,
    pub deck_name: String,
    /// Field values in note-type field order.
    pub fields: Vec<String>,
    /// Question template (e.g. `{{Front}}` or `{{cloze:Text}}`).
    pub qfmt: String,
    /// Answer template (e.g. `{{Answer}}` or `{{cloze:Text}}<br>{{Back Extra}}`).
    pub afmt: String,
    /// True when the note type is the cloze model.
    pub is_cloze: bool,
}

// ---------------------------------------------------------------------------
// Field map + filters
// ---------------------------------------------------------------------------

/// Build the `{{name}}` → value map. Field names are case-insensitive per Anki.
fn field_map(fields: &[String], field_names: &[String]) -> Vec<(String, String)> {
    field_names
        .iter()
        .cloned()
        .zip(fields.iter().cloned())
        .collect()
}

fn lookup<'m>(map: &'m [(String, String)], name: &str) -> Option<&'m str> {
    map.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Render `[sound:file.mp3]` → `<audio>` element; count occurrences.
fn render_sound(html: &str, flags: &mut FidelityFlags) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    loop {
        match rest.find("[sound:") {
            Some(i) => {
                let after = &rest[i..];
                let end = match after.find(']') {
                    Some(e) => i + e,
                    None => {
                        out.push_str(rest);
                        break;
                    }
                };
                let file = &rest[i + 7..end];
                out.push_str(&rest[..i]);
                out.push_str(&format!(
                    r#"<audio controls preload="none" src="/api/v1/reviewer/media/{file}" class="ankitov-audio"></audio>"#
                ));
                flags.audio = true;
                rest = &rest[end + 1..];
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

fn detect_flags(html: &str, flags: &mut FidelityFlags) {
    if html.contains("\\(") || html.contains("\\[") || html.contains("\\frac") {
        flags.mathjax = true;
    }
    if html.contains("<img") {
        flags.images = true;
    }
    // RTL heuristic: any Hebrew/Arabic block character in the rendered content.
    if html.chars().any(|c| {
        ('\u{0590}'..='\u{05FF}').contains(&c) || ('\u{0600}'..='\u{06FF}').contains(&c)
    }) {
        flags.rtl = true;
    }
}

// ---------------------------------------------------------------------------
// Template engine
// ---------------------------------------------------------------------------

/// Render one template string against the field map.
///
/// `cloze_ord` is the active deletion index for cloze cards (card.ord, 0-based);
/// ignored for non-cloze templates.
fn render_template(
    template: &str,
    map: &[(String, String)],
    is_cloze: bool,
    cloze_ord: i32,
    flags: &mut FidelityFlags,
) -> String {
    // 1) conditionals {{#Field}}...{{/Field}} and {{^Field}}...{{/Field}}
    // (hand-rolled scan; nested conditionals are not in the v1 subset)
    let mut out = template.to_string();
    out = render_conditionals(&out, map, "#", true);
    out = render_conditionals(&out, map, "^", false);

    // 2) cloze markup embedded directly in the TEMPLATE (rare; values are
    //    pre-rendered in render_card — this catches inline c-markup)
    if is_cloze {
        out = render_cloze(&out, cloze_ord, false);
    }

    // 3) {{type:...}} → typed-input placeholder
    let type_re = regex::Regex::new(r"\{\{type:(?:cloze:)?([^}]+)\}\}").unwrap();
    if type_re.is_match(&out) {
        flags.type_answer = true;
        out = type_re
            .replace_all(&out, r#"<input class="ankitov-typein" autocomplete="off" inputmode="text">"#)
            .to_string();
    }

    // 4) known filters: hint:, text:
    let hint_re = regex::Regex::new(r"\{\{hint:([^}]+)\}\}").unwrap();
    out = hint_re
        .replace_all(&out, |caps: &regex::Captures| {
            let name = caps.get(1).unwrap().as_str();
            match lookup(map, name) {
                Some(v) if !v.trim().is_empty() => format!(
                    r##"<a class="hint" href="#" onclick="this.nextElementSibling.style.display='block';this.style.display='none';return false;">hint</a><div class="hint" style="display:none">{}</div>"##,
                    v
                ),
                _ => String::new(),
            }
        })
        .to_string();
    let text_re = regex::Regex::new(r"\{\{text:([^}]+)\}\}").unwrap();
    out = text_re
        .replace_all(&out, |caps: &regex::Captures| {
            lookup(map, caps.get(1).unwrap().as_str()).unwrap_or("").to_string()
        })
        .to_string();

    // 5) plain {{Field}} replacement + unknown-filter detection
    let field_re = regex::Regex::new(r"\{\{([^}]+)\}\}").unwrap();
    out = field_re
        .replace_all(&out, |caps: &regex::Captures| {
            let raw = caps.get(1).unwrap().as_str();
            if let Some(name) = raw.strip_prefix("cloze:") {
                // {{cloze:Field}} — values are already cloze-rendered (render_card)
                return lookup(map, name).unwrap_or("").to_string();
            }
            if raw.contains(':') {
                // unknown filter (furigana/kanji/kana/...) — v1 renders empty + flags
                flags.unsupported_filter = true;
                return String::new();
            }
            if raw.eq_ignore_ascii_case("FrontSide") {
                // on the answer template, Anki embeds the question — v1 flags it
                flags.unsupported_filter = true;
                return String::new();
            }
            lookup(map, raw).unwrap_or("").to_string()
        })
        .to_string();

    // 6) media + flags
    let out = render_sound(&out, flags);
    detect_flags(&out, flags);
    out
}

/// Conditional blocks: `{{#Field}}body{{/Field}}` kept when the field has
/// non-empty content; `{{^Field}}body{{/Field}}` kept when it is empty.
/// Non-nested (v1 subset); unmatched closers render as empty.
fn render_conditionals(input: &str, map: &[(String, String)], marker: &str, keep_when_present: bool) -> String {
    let open_re = regex::Regex::new(&format!(
        r"\{{\{{{}\s*([^}}]+?)\s*\}}\}}",
        regex::escape(marker)
    ))
    .unwrap();
    let close_re = regex::Regex::new(&format!(r"\{{\{{/\s*([^}}]+?)\s*\}}\}}")).unwrap();

    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        let open_match = open_re.captures(rest);
        let Some(om) = open_match else {
            out.push_str(rest);
            break;
        };
        let start = om.get(0).unwrap().start();
        let name = om.get(1).unwrap().as_str().trim().to_string();
        out.push_str(&rest[..start]);
        let after_open = om.get(0).unwrap().end();
        let remainder = &rest[after_open..];
        // find the matching closer for the SAME name
        let close_pattern = format!(r"\{{\{{/\s*{}\s*\}}\}}", regex::escape(&name));
        let close_re_named = regex::Regex::new(&close_pattern).unwrap();
        match close_re_named.captures(remainder) {
            Some(cm) => {
                let body = &remainder[..cm.get(0).unwrap().start()];
                let value = lookup(map, &name).unwrap_or("");
                let present = !value.trim().is_empty();
                if present == keep_when_present {
                    out.push_str(body);
                }
                rest = &remainder[cm.get(0).unwrap().end()..];
            }
            None => {
                // unclosed conditional: drop the marker, keep scanning
                rest = remainder;
            }
        }
    }
    out
}

/// `{{cloze:Text}}` — render the active deletion for `cloze_ord`.
/// Deletions are 1-based in the markup (`{{c1::..}}`); card ord 0 → c1.
/// Question side (`reveal=false`): the active deletion renders as
/// `<span class="cloze">[...]</span>`. Answer side (`reveal=true`): the active
/// deletion's content is shown (Anki reveals it on the answer).
/// Non-active deletions always render their hidden text.
fn render_cloze(template: &str, cloze_ord: i32, reveal: bool) -> String {
    let active = cloze_ord + 1;
    let re = regex::Regex::new(r"\{\{c(\d+)::(.*?)\}\}").unwrap();
    re.replace_all(template, |caps: &regex::Captures| {
        let n: i32 = caps.get(1).unwrap().as_str().parse().unwrap_or(0);
        let content = caps.get(2).unwrap().as_str();
        if n == active {
            if reveal {
                content.to_string()
            } else {
                format!(r#"<span class="cloze">[...]</span>"#)
            }
        } else {
            content.to_string()
        }
    })
    .to_string()
}

/// Render a full card (question + answer) from raw collection data.
pub fn render_card(raw: &RawCard, field_names: &[String]) -> RenderedCard {
    let mut q_flags = FidelityFlags::none();
    let mut a_flags = FidelityFlags::none();
    let mut map = field_map(&raw.fields, field_names);
    // Cloze deletions live in the FIELD VALUES — render them there BEFORE
    // template substitution, so `{{cloze:Text}}` and plain `{{Text}}` both
    // insert already-resolved HTML. Two views: question hides the active
    // deletion as [...]; answer reveals it (Anki semantics).
    let mut answer_map = map.clone();
    if raw.is_cloze {
        for (_, v) in map.iter_mut().skip(2) {
            *v = render_cloze(v, raw.card_ord, false);
        }
        for (_, v) in answer_map.iter_mut().skip(2) {
            *v = render_cloze(v, raw.card_ord, true);
        }
    }

    let q_html = render_template(&raw.qfmt, &map, raw.is_cloze, raw.card_ord, &mut q_flags);
    let a_html = render_template(&raw.afmt, &answer_map, raw.is_cloze, raw.card_ord, &mut a_flags);

    // merge flags (answer side wins for unsupported/type; rtl/audio/images OR-ed)
    let flags = FidelityFlags {
        mathjax: q_flags.mathjax || a_flags.mathjax,
        unsupported_filter: q_flags.unsupported_filter || a_flags.unsupported_filter,
        audio: q_flags.audio || a_flags.audio,
        images: q_flags.images || a_flags.images,
        type_answer: q_flags.type_answer || a_flags.type_answer,
        rtl: q_flags.rtl || a_flags.rtl,
    };

    RenderedCard {
        card_id: raw.card_id,
        note_id: raw.note_id,
        question: RenderedSide { html: q_html },
        answer: RenderedSide { html: a_html },
        flags,
        deck_name: raw.deck_name.clone(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        vec!["Front".into(), "Back".into(), "Text".into(), "Back Extra".into()]
    }

    fn raw(qfmt: &str, afmt: &str, fields: Vec<&str>, is_cloze: bool, ord: i32) -> RawCard {
        RawCard {
            card_id: 1,
            note_id: 1,
            card_ord: ord,
            deck_name: "Test".into(),
            fields: fields.into_iter().map(String::from).collect(),
            qfmt: qfmt.into(),
            afmt: afmt.into(),
            is_cloze,
        }
    }

    #[test]
    fn basic_replacement() {
        let c = render_card(
            &raw("{{Front}}", "{{Front}}<hr>{{Back}}", vec!["What is a cell?", "The smallest unit of life"], false, 0),
            &names(),
        );
        assert_eq!(c.question.html, "What is a cell?");
        assert!(c.answer.html.contains("The smallest unit of life"));
        assert!(!c.flags.rtl && !c.flags.mathjax);
    }

    #[test]
    fn cloze_active_vs_inactive() {
        let c = render_card(
            &raw(
                "{{cloze:Text}}",
                "{{cloze:Text}}<br>{{Back Extra}}",
                vec!["", "", "The {{c1::cell theory}} states that {{c2::all cells}} come from cells.", "Schleiden & Schwann"],
                true,
                0,
            ),
            &names(),
        );
        assert!(c.question.html.contains(r#"<span class="cloze">[...]</span>"#));
        assert!(c.question.html.contains("all cells come from cells")); // c2 inactive → shown
        assert!(!c.question.html.contains("cell theory")); // c1 active → hidden
        assert!(c.answer.html.contains("cell theory")); // answer shows the deletion text
    }

    #[test]
    fn conditional_blocks() {
        let c = render_card(
            &raw("{{#Back}}{{Back}}{{/Back}}{{^Back}}no back{{/Back}}", "{{Front}}", vec!["F", ""], false, 0),
            &names(),
        );
        assert_eq!(c.question.html, "no back");
        let c2 = render_card(
            &raw("{{#Back}}{{Back}}{{/Back}}{{^Back}}no back{{/Back}}", "{{Front}}", vec!["F", "present"], false, 0),
            &names(),
        );
        assert_eq!(c2.question.html, "present");
    }

    #[test]
    fn sound_and_rtl_and_math() {
        let c = render_card(
            &raw("{{Front}}", "{{Front}}", vec!["שלום [sound:shalom.mp3] \\(x^2\\)", ""], false, 0),
            &names(),
        );
        assert!(c.flags.audio && c.flags.rtl && c.flags.mathjax);
        assert!(c.question.html.contains("/api/v1/reviewer/media/shalom.mp3"));
    }

    #[test]
    fn type_answer_and_unknown_filter() {
        let c = render_card(
            &raw("{{type:Back}}", "{{furigana:Front}}", vec!["f", "answer"], false, 0),
            &names(),
        );
        assert!(c.flags.type_answer);
        assert!(c.question.html.contains("ankitov-typein"));
        assert!(c.flags.unsupported_filter);
        assert!(c.answer.html.is_empty()); // unknown filter renders empty
    }
}
