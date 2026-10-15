use super::*;

#[test]
fn plan_limit_and_initial_policy_bounds_are_enforced() {
    let mut plan = crate::review_context::tests::plan();
    for limit in [0, MAX_BYTES + 1, u32::MAX as usize] {
        plan.0.limits.log_bytes = limit as u32;
        assert_eq!(BuildOutput::for_plan(&plan).err(), Some(InvalidLimit));
    }
    for limit in [1, 37, MAX_BYTES] {
        plan.0.limits.log_bytes = limit as u32;
        let output = BuildOutput::for_plan(&plan).unwrap();
        assert_eq!(output.limit, limit);
        assert_eq!(output.retained_bytes(), 0);
        assert!(!output.limit_reached());
        assert!(!output.truncated());
    }
}

#[test]
fn streams_share_one_budget_and_truncation_is_sticky() {
    let mut output = BuildOutput::new(5).unwrap();
    assert_eq!(
        output.append(Stream::Stdout, b"abc"),
        Append {
            retained: 3,
            limit_reached: false
        }
    );
    assert_eq!(
        output.append(Stream::Stderr, b"defg"),
        Append {
            retained: 2,
            limit_reached: true
        }
    );
    assert_eq!(output.escaped_text(Stream::Stdout), "abc");
    assert_eq!(output.escaped_text(Stream::Stderr), "de");
    assert!(output.truncated());
    for stream in [Stream::Stdout, Stream::Stderr] {
        assert_eq!(
            output.append(stream, b"more"),
            Append {
                retained: 0,
                limit_reached: true
            }
        );
        assert_eq!(
            output.append(stream, b""),
            Append {
                retained: 0,
                limit_reached: true
            }
        );
    }
    assert_eq!(output.retained_bytes(), 5);
    assert!(output.truncated());
}

#[test]
fn exact_capacity_requires_stop_without_claiming_discarded_bytes() {
    let mut output = BuildOutput::new(1).unwrap();
    assert!(!output.append(Stream::Stderr, b"").limit_reached);
    assert!(output.append(Stream::Stdout, b"x").limit_reached);
    assert!(!output.truncated());
    assert!(output.append(Stream::Stderr, b"").limit_reached);
    assert!(!output.truncated());
    output.append(Stream::Stderr, b"y");
    assert!(output.truncated());
}

#[test]
fn utf8_is_decoded_per_stream_independent_of_chunk_boundaries() {
    let text = "hello é 🦀\n";
    for split in 0..=text.len() {
        let mut output = BuildOutput::new(100).unwrap();
        output.append(Stream::Stdout, &text.as_bytes()[..split]);
        let _interim = output.escaped_text(Stream::Stdout);
        output.append(Stream::Stderr, b"separate");
        output.append(Stream::Stdout, &text.as_bytes()[split..]);
        assert_eq!(output.escaped_text(Stream::Stdout), text);
        assert_eq!(output.escaped_text(Stream::Stderr), "separate");
    }
    let mut output = BuildOutput::new(20).unwrap();
    output.append(Stream::Stdout, &[0xc3]);
    output.append(Stream::Stderr, &[0xa9]);
    assert_eq!(output.escaped_text(Stream::Stdout), "�");
    assert_eq!(output.escaped_text(Stream::Stderr), "�");
}

#[test]
fn invalid_and_capacity_split_utf8_have_explicit_replacement() {
    let mut output = BuildOutput::new(3).unwrap();
    output.append(Stream::Stdout, "🦀".as_bytes());
    assert_eq!(output.escaped_text(Stream::Stdout), "�");
    assert!(output.truncated());
    let mut output = BuildOutput::new(10).unwrap();
    output.append(Stream::Stderr, &[0xff, b'a', 0xc3]);
    assert_eq!(output.escaped_text(Stream::Stderr), "�a�");
}

#[test]
fn markup_terminal_controls_and_literal_escape_text_stay_inert() {
    let mut output = BuildOutput::new(100).unwrap();
    output.append(
        Stream::Stdout,
        b"<script>&\"'\x1b[31mred\r\x08\0\x7f\\u{1b}\n\t",
    );
    assert_eq!(
        output.escaped_text(Stream::Stdout),
        "&lt;script&gt;&amp;&quot;&#39;\\u{1b}[31mred\\u{d}\\u{8}\\u{0}\\u{7f}\\\\u{1b}\n\t"
    );
    assert!(!output.truncated());
}

#[test]
fn unicode_controls_and_bidirectional_formatting_are_visible() {
    let input = "\u{85}\u{9b}\u{61c}\u{200e}\u{200f}\u{202a}\u{202b}\u{202c}\u{202d}\u{202e}\u{2066}\u{2067}\u{2068}\u{2069}";
    let mut output = BuildOutput::new(100).unwrap();
    output.append(Stream::Stdout, input.as_bytes());
    let expected: String = input
        .chars()
        .map(|c| format!("\\u{{{:x}}}", c as u32))
        .collect();
    assert_eq!(output.escaped_text(Stream::Stdout), expected);
}

#[test]
fn arbitrary_bytes_and_maximum_input_respect_storage_and_render_bounds() {
    let bytes: Vec<_> = (0..=255).collect();
    let mut output = BuildOutput::new(MAX_BYTES).unwrap();
    for _ in 0..MAX_BYTES / bytes.len() {
        output.append(Stream::Stdout, &bytes);
    }
    assert!(output.limit_reached());
    assert!(!output.truncated());
    let rendered = output.escaped_text(Stream::Stdout);
    assert!(rendered.len() <= 6 * MAX_BYTES);
    assert!(
        !rendered
            .chars()
            .any(|c| (c.is_control() && c != '\n' && c != '\t') || bidi_format(c))
    );
    assert!(!rendered.contains(['<', '>', '"', '\'']));
    let capacity = (output.stdout.capacity(), output.stderr.capacity());
    output.append(Stream::Stderr, &vec![0; MAX_BYTES * 2]);
    assert_eq!(output.retained_bytes(), MAX_BYTES);
    assert_eq!(
        capacity,
        (output.stdout.capacity(), output.stderr.capacity())
    );
    assert!(output.truncated());
}
