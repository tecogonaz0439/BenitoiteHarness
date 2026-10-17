//! 取得先のホスト名を設定と URL で同じ規則に揃える（設計書 01-03「web_fetch」）。

pub(crate) fn normalize(input: &str) -> Option<String> {
    let domain = input.to_ascii_lowercase();
    let domain = domain.strip_suffix('.').unwrap_or(&domain);
    if !domain.contains('.') || domain.len() > 253 {
        return None;
    }
    for label in domain.split('.') {
        if label.is_empty()
            || label.len() > 63
            || !label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return None;
        }
    }
    let last = domain.rsplit('.').next()?;
    if last.bytes().all(|b| b.is_ascii_digit()) || last.starts_with("0x") {
        return None;
    }
    Some(domain.to_owned())
}
