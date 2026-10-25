use super::shell_word;

#[test]
fn a_token_of_safe_characters_is_not_quoted() {
    let token = "github.com/gorilla/mux@98cb6bf42e08:mux.go";
    assert_eq!(shell_word(token), token);
}
