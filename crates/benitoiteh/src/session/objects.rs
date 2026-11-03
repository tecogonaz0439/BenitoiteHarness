//! ハッシュ名の中身と一覧を、容量を予約して保存する（設計書 01-05「巻き戻し」）。

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::sync::Arc;

use rustix::fs::{AtFlags, OFlags};
use sha2::{Digest, Sha256};

use super::{Session, SessionError, create_file, id, open_existing_file, text};

/// ハッシュごとに一度だけ読んで確かめた中身と、形式と寸法。
type RestoredObjects = HashMap<String, Option<(Arc<[u8]>, &'static str, u32, u32)>>;

impl Session {
    /// 再開の会話の画像をハッシュで確かめ、戻せない画像には断りの文を残す。
    pub(crate) fn restore_images(
        &self,
        conversation: &mut crate::provider::Conversation,
        image_input: bool,
    ) {
        if !image_input {
            return;
        }
        // 同じハッシュの成功・失敗を共に貯め、記録の参照数に応じた読み直しと
        // 中身の複製を避ける。記録の属性は参照ごとに照合する（設計書 01-05「記録を残す期間」）。
        let mut objects = HashMap::new();
        for item in &mut conversation.items {
            match item {
                crate::provider::Item::ToolResult(result) => {
                    if self.restore_list(&mut result.images, &mut objects) {
                        result.text.push('\n');
                        result.text.push_str(text::IMAGE_RESTORE_OMITTED);
                    }
                }
                // 利用者の添付も同じ確かめで戻す（設計書 02-01「画像の受け渡し」）。
                crate::provider::Item::UserMessage(message) => {
                    if self.restore_list(&mut message.images, &mut objects) {
                        message.parts.push(crate::provider::UserPart {
                            kind: crate::provider::UserPartKind::Request,
                            text: text::ATTACHMENT_RESTORE_OMITTED.to_owned(),
                        });
                    }
                }
                crate::provider::Item::AssistantTurn(_) => {}
            }
        }
    }

    /// 画像の並びを中身で確かめて戻し、戻せないものを除く。除いたら真を返す。
    fn restore_list(
        &self,
        images: &mut Vec<crate::provider::ToolImage>,
        objects: &mut RestoredObjects,
    ) -> bool {
        let mut omitted = false;
        images.retain_mut(|image| {
            let restored = objects.entry(image.sha256.clone()).or_insert_with(|| {
                let bytes = self
                    .read_object(&image.sha256, crate::tools::MAX_IMAGE_BYTES)
                    .ok()?;
                let (mime, width, height) = crate::tools::inspect_image(&bytes).ok()?;
                Some((Arc::<[u8]>::from(bytes), mime, width, height))
            });
            match restored.as_ref() {
                Some((bytes, mime, width, height))
                    if u64::try_from(bytes.len()).ok() == Some(image.bytes)
                        && *mime == image.media_type
                        && *width == image.width
                        && *height == image.height =>
                {
                    image.data = Some(Arc::clone(bytes));
                    true
                }
                _ => {
                    omitted = true;
                    false
                }
            }
        });
        omitted
    }

    // 保存した名前の再確認は省き、復元時には必ず中身をハッシュと照合する。
    pub(crate) fn object_known(&self, hash: &str) -> bool {
        self.inner
            .known_objects
            .lock()
            .is_ok_and(|known| known.contains(hash))
    }
    fn remember_object(&self, hash: &str) {
        if let Ok(mut known) = self.inner.known_objects.lock() {
            known.insert(hash.to_owned());
        }
    }

    /// SHA-256 名で一度だけ保存する。容量不足は None、値は名前と新規保存の有無。
    pub fn store_object(&self, bytes: &[u8]) -> Result<Option<(String, bool)>, SessionError> {
        let hash = id::hex(&Sha256::digest(bytes));
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        match self.open_object(&hash, size) {
            Ok(file) => {
                if file.metadata()?.len() != size {
                    return Err(SessionError::Invalid(text::INVALID_OBJECT));
                }
                self.remember_object(&hash);
                return Ok(Some((hash, false)));
            }
            Err(SessionError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if !self.reserve(size) {
            return Ok(None);
        }
        let mut random = [0; 16];
        if let Err(error) = getrandom::fill(&mut random) {
            self.release(size);
            return Err(SessionError::Random(error));
        }
        let temporary = format!("tmp-{}", id::hex(&random));
        let result = (|| {
            let mut file = create_file(&self.inner.objects, &temporary, OFlags::WRONLY)?;
            file.write_all(bytes)?;
            file.flush()?;
            rustix::fs::renameat(&self.inner.objects, &temporary, &self.inner.objects, &hash)?;
            self.remember_object(&hash);
            Ok(Some((hash, true)))
        })();
        if result.is_err() {
            let _removed = rustix::fs::unlinkat(&self.inner.objects, &temporary, AtFlags::empty());
            self.release(size);
        }
        result
    }

    /// 信頼しない中身を NOFOLLOW・NONBLOCK で開き、通常ファイル、単独リンク、長さを確かめる。
    pub fn open_object(&self, hash: &str, limit: u64) -> Result<File, SessionError> {
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(SessionError::Invalid(text::INVALID_OBJECT));
        }
        let file = open_existing_file(&self.inner.objects, hash, OFlags::RDONLY)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > limit {
            return Err(SessionError::Invalid(text::INVALID_OBJECT));
        }
        Ok(file)
    }

    /// 一覧を上限付きで読み、名前の SHA-256 と照合する。
    pub fn read_object(&self, hash: &str, limit: u64) -> Result<Vec<u8>, SessionError> {
        let file = self.open_object(hash, limit)?;
        let mut bytes = Vec::new();
        file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit
            || id::hex(&Sha256::digest(&bytes)) != hash
        {
            return Err(SessionError::Invalid(text::INVALID_OBJECT));
        }
        self.remember_object(hash);
        Ok(bytes)
    }
}
