use super::date;

#[test]
fn a_date_is_the_first_ten_characters_of_a_timestamp() {
    assert_eq!(date("2026-09-14T10:19:56Z"), "2026-09-14");
    assert_eq!(date("2026-09"), "2026-09");
    assert_eq!(
        date(
            "\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}"
        ),
        "\u{e9}".repeat(10)
    );
}
