//! Skill の本文の抽出の境界を確かめる（設計書 01-04「Benitoite の Skill の置き方」）。

use super::{ContextError, skill_parts};

#[test]
fn skill_body_removes_only_the_first_leading_comment() -> Result<(), ContextError> {
    for (body, expected) in [
        ("\n# Benitoite\n", "\n# Benitoite\n"),
        (
            "\n# Benitoite\n<!-- keep -->\ntext\n",
            "\n# Benitoite\n<!-- keep -->\ntext\n",
        ),
        ("\n<!-- 説明 -->\n\n# Benitoite\n", "\n# Benitoite\n"),
        (
            "\n \t\n<!-- explanation -->\r\n# Benitoite\r\n",
            "# Benitoite\r\n",
        ),
        (
            "<!-- first -->\n<!-- keep -->\n# Benitoite\n",
            "<!-- keep -->\n# Benitoite\n",
        ),
        ("<!-- explanation --># Benitoite", "# Benitoite"),
        ("<!-- explanation -->", ""),
        ("# Benitoite\n<!-- unclosed", "# Benitoite\n<!-- unclosed"),
    ] {
        let skill = format!("---\nname: benitoite\n---\n{body}");
        let (attributes, actual) = skill_parts(&skill)?;
        assert_eq!(attributes, "name: benitoite");
        assert_eq!(actual, expected, "{body:?}");
    }
    for body in ["<!--", "\n<!-- unclosed\n# Benitoite\n"] {
        let skill = format!("---\nname: benitoite\n---\n{body}");
        assert!(matches!(
            skill_parts(&skill),
            Err(ContextError::InvalidSkill)
        ));
    }
    Ok(())
}
