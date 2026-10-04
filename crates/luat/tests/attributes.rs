// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Dynamic attribute values: `nil` and `false` omit the attribute, `true`
//! renders it bare, everything else is escaped. Spreads follow the same
//! rules.

use std::collections::HashMap;

use luat::memory_resolver::MemoryResourceResolver;
use luat::Engine;

fn render(template: &str) -> String {
    let engine = Engine::with_memory_cache(MemoryResourceResolver::new(), 4).unwrap();
    engine.render_source(template, &HashMap::new()).unwrap()
}

#[test]
fn nil_and_false_omit_the_attribute() {
    assert_eq!(
        render("<script>local t = nil</script><a href={t} title={false} rel={1 > 2 and 'x'}>a</a>"),
        "<a>a</a>"
    );
    assert_eq!(render("<input value=\"{nil}\" />"), "<input />");
}

#[test]
fn true_renders_a_bare_attribute() {
    assert_eq!(
        render("<button disabled={true} hidden={false}>b</button>"),
        "<button disabled>b</button>"
    );
    assert_eq!(render("<input checked={1 < 2} />"), "<input checked />");
}

#[test]
fn true_and_false_valued_attributes_keep_true() {
    assert_eq!(
        render(
            "<div aria-hidden={true} data-open={true} draggable={true} aria-busy={false}>x</div>"
        ),
        "<div aria-hidden=\"true\" data-open=\"true\" draggable=\"true\">x</div>"
    );
}

#[test]
fn strings_and_numbers_are_escaped() {
    assert_eq!(
        render("<a title={'a \"b\" <c>'} tabindex={0} alt={''}>x</a>"),
        "<a title=\"a &quot;b&quot; &lt;c&gt;\" tabindex=\"0\" alt=\"\">x</a>"
    );
    assert_eq!(
        render("<a title=\"n={1 + 1}\">x</a>"),
        "<a title=\"n=2\">x</a>"
    );
}

#[test]
fn class_follows_the_same_rules() {
    assert_eq!(render("<p class={false}>x</p>"), "<p>x</p>");
    assert_eq!(render("<p class={nil}>x</p>"), "<p>x</p>");
    assert_eq!(render("<p class={'a b'}>x</p>"), "<p class=\"a b\">x</p>");
    assert_eq!(
        render("<p class={{ on = true, off = false }}>x</p>"),
        "<p class=\"on\">x</p>"
    );
}

#[test]
fn raw_attribute_values_are_omitted_when_nil() {
    assert_eq!(render("<p data-x={@html nil}>x</p>"), "<p>x</p>");
    assert_eq!(
        render("<p data-x={@html '<b>'}>x</p>"),
        "<p data-x=\"<b>\">x</p>"
    );
}

#[test]
fn spreads_follow_the_same_rules() {
    let html = render(
        "<script>local attrs = { disabled = true, hidden = false, title = 'a&b', id = nil }</script><button {...attrs}>b</button>",
    );
    // `pairs` order is unspecified.
    assert!(
        html == "<button disabled title=\"a&amp;b\">b</button>"
            || html == "<button title=\"a&amp;b\" disabled>b</button>",
        "{html}"
    );
}

#[test]
fn spreads_skip_invalid_attribute_names() {
    let html = render(
        "<script>local attrs = { 'positional', ['x\"><script>'] = 'v', ['a b'] = 'v', ok = 'y' }</script><p {...attrs}>x</p>",
    );
    assert_eq!(html, "<p ok=\"y\">x</p>");
}

#[test]
fn nil_spread_adds_nothing() {
    assert_eq!(
        render("<script>local t = nil</script><a {...t} href=\"/\">a</a>"),
        "<a href=\"/\">a</a>"
    );
    assert_eq!(render("<script>local t = {}</script><a {...t.missing}>a</a>"), "<a>a</a>");
}
