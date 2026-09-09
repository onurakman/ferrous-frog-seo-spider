use super::*;

fn rule(kind: ExtractorKind, pattern: &str) -> CustomExtractor {
    CustomExtractor {
        name: "sample".into(),
        kind,
        pattern: pattern.into(),
        attribute: Some("data-value".into()),
        all_matches: true,
    }
}

#[test]
fn preview_preserves_each_extraction_kind_and_first_match_semantics() {
    let html = "<html><body><p data-value=\" café   tea \"> One <b>two</b> </p><p data-value=\" \"> </p><p data-value=\"last\"> Three </p></body></html>";
    for (kind, pattern, expected) in [
        (ExtractorKind::CssText, "p", vec!["One two", "Three"]),
        (ExtractorKind::CssAttribute, "p", vec!["café tea", "last"]),
        (
            ExtractorKind::Regex,
            r#"data-value="([^"]*)""#,
            vec![" café   tea ", " ", "last"],
        ),
        (ExtractorKind::XPath, "//p", vec!["One two", "Three"]),
    ] {
        let mut extractor = rule(kind, pattern);
        for all_matches in [true, false] {
            extractor.all_matches = all_matches;
            let result = preview_extractor(html, &extractor).unwrap();
            let expected = if all_matches {
                expected.clone()
            } else {
                vec![expected[0]]
            };
            assert_eq!(result.values, expected, "{extractor:?}");
            assert!(!result.values_truncated);
            assert!(!result.text_truncated);
            assert_eq!(
                run_extractors(html, std::slice::from_ref(&extractor)).unwrap()[0].values,
                expected
            );
        }
    }
    for (pattern, expected) in [
        ("string(//missing)", ""),
        ("count(//p)", "3"),
        ("boolean(//p)", "true"),
    ] {
        let result = preview_extractor(html, &rule(ExtractorKind::XPath, pattern)).unwrap();
        assert_eq!(result.values, [expected]);
    }
    let result = preview_extractor("b", &rule(ExtractorKind::Regex, "(a)?b")).unwrap();
    assert_eq!(result.values, ["b"]);
}

#[test]
fn preview_caps_all_match_collection_without_changing_crawl_output() {
    for (kind, pattern) in [
        (ExtractorKind::CssText, "p"),
        (ExtractorKind::CssAttribute, "p"),
        (ExtractorKind::Regex, r#"data-value="([^"]*)""#),
        (ExtractorKind::XPath, "//p"),
    ] {
        let mut extractor = rule(kind, pattern);
        for count in [100, 101, 1_000] {
            let html = format!(
                "<html><body>{}</body></html>",
                "<p data-value=\"value=x\">value=x</p>".repeat(count)
            );
            let result = preview_extractor(&html, &extractor).unwrap();
            assert_eq!(result.values.len(), 100, "{extractor:?}");
            assert_eq!(result.values_truncated, count > 100);
            assert!(!result.text_truncated);
            let crawl = run_extractors(&html, std::slice::from_ref(&extractor)).unwrap();
            assert_eq!(crawl[0].values.len(), count);
        }
        extractor.all_matches = false;
        let result = preview_extractor(
            "<html><p data-value=\"value=x\">value=x</p><p data-value=\"value=y\">value=y</p></html>",
            &extractor,
        )
        .unwrap();
        assert_eq!(result.values.len(), 1);
        assert!(!result.values_truncated);
    }
}

#[test]
fn preview_stops_consuming_match_and_text_iterators_at_its_bounds() {
    for (all_matches, expected_visits) in [(true, 101), (false, 1)] {
        let mut visited = 0;
        let values = std::iter::repeat_with(|| {
            visited += 1;
            assert!(visited <= expected_visits, "preview consumed extra matches");
            "value".to_string()
        });
        let result = collect_values(values, all_matches, true);
        assert_eq!(result.values.len(), if all_matches { 100 } else { 1 });
        assert_eq!(visited, expected_visits);
        assert_eq!(result.values_truncated, all_matches);
    }

    let mut visited = 0;
    let parts = std::iter::repeat_with(|| {
        visited += 1;
        assert!(visited <= 1_001, "preview traversed extra CSS text nodes");
        "x"
    });
    let text = extraction_text(parts, true);
    assert_eq!(text.chars().count(), 2_001);
    assert_eq!(visited, 1_001);
}

#[test]
fn preview_truncates_unicode_values_after_normalization() {
    let text = "界".repeat(2_001);
    let html = format!("<html><p data-value=\"  {text}  \">  {text}  </p></html>");
    for (kind, pattern) in [
        (ExtractorKind::CssText, "p"),
        (ExtractorKind::CssAttribute, "p"),
        (ExtractorKind::Regex, "(界+)"),
        (ExtractorKind::XPath, "//p"),
        (ExtractorKind::XPath, "string(//p)"),
    ] {
        let extractor = rule(kind, pattern);
        let result = preview_extractor(&html, &extractor).unwrap();
        assert!(result.text_truncated, "{extractor:?}");
        assert!(!result.values_truncated);
        assert!(
            result
                .values
                .iter()
                .all(|value| value == &"界".repeat(2_000))
        );
        assert!(
            run_extractors(&html, &[extractor]).unwrap()[0]
                .values
                .iter()
                .all(|value| value == &text)
        );
    }
    let result = preview_extractor(
        &format!("<p>{}</p>", "界".repeat(2_000)),
        &rule(ExtractorKind::CssText, "p"),
    )
    .unwrap();
    assert!(!result.text_truncated);
}

#[test]
fn preview_distinguishes_no_matches_from_invalid_rules_and_xpath_samples() {
    for (kind, pattern) in [
        (ExtractorKind::CssText, "missing"),
        (ExtractorKind::CssAttribute, "missing"),
        (ExtractorKind::Regex, "missing"),
        (ExtractorKind::XPath, "//missing"),
    ] {
        let result = preview_extractor("<html/>", &rule(kind, pattern)).unwrap();
        assert!(result.values.is_empty());
        assert!(!result.values_truncated);
        assert!(!result.text_truncated);
    }
    for (kind, pattern, message) in [
        (ExtractorKind::CssText, "[", "invalid CSS selector"),
        (ExtractorKind::Regex, "(", "invalid regex"),
        (ExtractorKind::XPath, "//[", "invalid XPath"),
    ] {
        let error = preview_extractor("<html/>", &rule(kind, pattern)).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
    let error = preview_extractor(
        "<html><img src=\"x\"></html>",
        &rule(ExtractorKind::XPath, "//img/@src"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("invalid XML/HTML document"));
}

#[test]
fn preview_rejects_oversized_samples_and_rule_fields_before_parsing() {
    let mut extractor = rule(ExtractorKind::Regex, "absent");
    assert!(preview_extractor(&"é".repeat(256 * 1024), &extractor).is_ok());
    let error = preview_extractor(&"é".repeat(256 * 1024 + 1), &extractor).unwrap_err();
    assert!(error.to_string().contains("512 KiB"));

    extractor.name = "界".repeat(200);
    extractor.pattern = "界".repeat(2_000);
    extractor.attribute = Some("界".repeat(200));
    assert!(preview_extractor("", &extractor).is_ok());
    for field in ["name", "pattern", "attribute"] {
        let mut invalid = extractor.clone();
        match field {
            "name" => invalid.name.push('界'),
            "pattern" => invalid.pattern.push('界'),
            _ => invalid.attribute.as_mut().unwrap().push('界'),
        }
        let error = preview_extractor("", &invalid).unwrap_err().to_string();
        assert!(error.contains(field), "{error}");
        assert!(
            !error.contains('界'),
            "limit errors must not echo rule values"
        );
    }
}
