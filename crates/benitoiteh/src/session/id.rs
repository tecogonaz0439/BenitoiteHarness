//! UTC と乱数によるセッション ID（設計書 05-01「データの置き場所」）。

use std::fmt;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{SessionError, text};

/// 時刻順に並び、パスの一要素としても使えるセッション ID。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SessionId {
    value: String,
    seconds: u64,
}

impl SessionId {
    /// UTC の秒と OS の乱数 8 バイトから ID を作る。
    pub fn generate(started: SystemTime) -> Result<Self, SessionError> {
        let seconds = started
            .duration_since(UNIX_EPOCH)
            .map_err(|_| SessionError::Invalid(text::INVALID_TIME))?
            .as_secs();
        let stamp = timestamp(seconds)?;
        let mut random = [0; 8];
        getrandom::fill(&mut random).map_err(SessionError::Random)?;
        Ok(Self {
            value: format!("{stamp}-{}", hex(&random)),
            seconds,
        })
    }

    /// 検証した名前を借りる。
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// ID が示す UTC の開始時刻。
    pub fn started_at(&self) -> SystemTime {
        UNIX_EPOCH
            .checked_add(Duration::from_secs(self.seconds))
            .unwrap_or(UNIX_EPOCH)
    }

    /// ヘッダに使う UTC の時刻（YYYY-MM-DDTHH:MM:SSZ）。
    pub fn started_at_utc(&self) -> String {
        // 構文と日付を構築時に確かめた ASCII だけを分割する。
        let stamp = self.value.get(..16).unwrap_or_default();
        format!(
            "{}-{}-{}T{}:{}:{}Z",
            stamp.get(..4).unwrap_or_default(),
            stamp.get(4..6).unwrap_or_default(),
            stamp.get(6..8).unwrap_or_default(),
            stamp.get(9..11).unwrap_or_default(),
            stamp.get(11..13).unwrap_or_default(),
            stamp.get(13..15).unwrap_or_default(),
        )
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

impl FromStr for SessionId {
    type Err = SessionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || SessionError::Invalid(text::INVALID_ID);
        if value.len() != 33
            || value.get(8..9) != Some("T")
            || value.get(15..17) != Some("Z-")
            || !value.get(17..).is_some_and(|part| {
                part.bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
        {
            return Err(invalid());
        }
        let number = |range: std::ops::Range<usize>| -> Result<u64, SessionError> {
            let part = value.get(range).ok_or_else(invalid)?;
            if !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid());
            }
            part.parse().map_err(|_| invalid())
        };
        let year = number(0..4)?;
        let month = number(4..6)?;
        let day = number(6..8)?;
        let hour = number(9..11)?;
        let minute = number(11..13)?;
        let second = number(13..15)?;
        if !(1970..=9999).contains(&year)
            || !(1..=12).contains(&month)
            || day == 0
            || day > month_days(year, month)
            || hour > 23
            || minute > 59
            || second > 59
        {
            return Err(invalid());
        }
        let mut days = 0_u64;
        for previous in 1970..year {
            days = days.saturating_add(year_days(previous));
        }
        for previous in 1..month {
            days = days.saturating_add(month_days(year, previous));
        }
        days = days.saturating_add(day.saturating_sub(1));
        let seconds = days
            .saturating_mul(86400)
            .saturating_add(hour.saturating_mul(3600))
            .saturating_add(minute.saturating_mul(60))
            .saturating_add(second);
        Ok(Self {
            value: value.to_owned(),
            seconds,
        })
    }
}

fn timestamp(seconds: u64) -> Result<String, SessionError> {
    let mut days = seconds / 86400;
    let mut year = 1970_u64;
    while days >= year_days(year) {
        days = days.saturating_sub(year_days(year));
        year = year.saturating_add(1);
        if year > 9999 {
            return Err(SessionError::Invalid(text::INVALID_TIME));
        }
    }
    let mut month = 1_u64;
    while days >= month_days(year, month) {
        days = days.saturating_sub(month_days(year, month));
        month = month.saturating_add(1);
    }
    let day = days.saturating_add(1);
    let hour = seconds % 86400 / 3600;
    let minute = seconds % 3600 / 60;
    let second = seconds % 60;
    Ok(format!(
        "{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z"
    ))
}

fn year_days(year: u64) -> u64 {
    if leap(year) { 366 } else { 365 }
}

fn leap(year: u64) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn month_days(year: u64, month: u64) -> u64 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if leap(year) => 29,
        2 => 28,
        _ => 31,
    }
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
