use openrust::{image::encoded_dimensions, support::Str};
use serde_json::Value;

use crate::{Attachment, DocumentAttachment, ImageAttachment, attachments::AttachmentBudget};

use super::{ContentBlock, attachment_block, chat_tools::has_only};

const MAX_IMAGE_BYTES: usize = 3_750_000;
const MAX_DOCUMENT_BYTES: usize = 4_500_000;
const MAX_DOCUMENTS: usize = 5;

#[derive(Default)]
pub(super) struct ChatMedia {
    budget: AttachmentBudget,
    documents: usize,
}

impl ChatMedia {
    pub(super) fn counting_attachment(
        &mut self,
        kind: &str,
        part: &Value,
    ) -> Result<ContentBlock, &'static str> {
        let source = &part["source"];
        if !has_only(part, &["type", "source"])
            || !has_only(source, &["type", "media_type", "data"])
            || source["type"] != "base64"
        {
            return Err("Bedrock 计数只支持内联 Base64 附件");
        }
        let mime = source["media_type"].as_str().ok_or("附件缺少 MIME 类型")?;
        let encoded = source["data"].as_str().ok_or("附件缺少 Base64 数据")?;
        if kind == "image" {
            return self.attachment(decode_image(mime, encoded)?.into());
        }
        self.documents += 1;
        if self.documents > MAX_DOCUMENTS {
            return Err("Bedrock 每次最多接收五份文档");
        }
        let filename = match mime {
            "application/pdf" => "document.pdf",
            "text/plain" => "document.txt",
            "text/markdown" => "document.md",
            _ => return Err("Bedrock 计数文档须为 PDF、UTF-8 文本或 Markdown"),
        };
        let document =
            DocumentAttachment::from_bytes(filename, decode(encoded, MAX_DOCUMENT_BYTES)?)
                .map_err(|_| "Bedrock 计数文档无效")?;
        if document.mime() != mime {
            return Err("Bedrock 计数文档格式须与声明一致");
        }
        self.attachment(document.into())
    }

    pub(super) fn user_content(
        &mut self,
        content: &Value,
    ) -> Result<Vec<ContentBlock>, &'static str> {
        if let Some(text) = content.as_str().filter(|text| !text.trim().is_empty()) {
            return Ok(vec![ContentBlock::Text(text.into())]);
        }
        let parts = content
            .as_array()
            .filter(|parts| !parts.is_empty())
            .ok_or("Bedrock Chat 用户消息须包含文本或附件")?;
        let mut blocks = Vec::with_capacity(parts.len());
        let mut has_text = false;
        let mut has_document = false;
        for part in parts {
            let block = match part["type"].as_str() {
                Some("text") if has_only(part, &["type", "text"]) => {
                    let text = part["text"]
                        .as_str()
                        .filter(|text| !text.trim().is_empty())
                        .ok_or("Bedrock Chat 文本块须为非空文本")?;
                    has_text = true;
                    ContentBlock::Text(text.into())
                }
                Some("image_url") if has_only(part, &["type", "image_url"]) => {
                    self.attachment(image(&part["image_url"])?.into())?
                }
                Some("file") if has_only(part, &["type", "file"]) => {
                    self.documents += 1;
                    if self.documents > MAX_DOCUMENTS {
                        return Err("Bedrock Chat 每次最多接收五份文档");
                    }
                    has_document = true;
                    self.attachment(document(&part["file"])?.into())?
                }
                _ => return Err("Bedrock Chat 仅支持用户文本及内联图片和文档"),
            };
            blocks.push(block);
        }
        if has_document && !has_text {
            return Err("Bedrock Chat 文档所在消息须同时包含文本说明");
        }
        Ok(blocks)
    }

    fn attachment(&mut self, attachment: Attachment) -> Result<ContentBlock, &'static str> {
        self.budget
            .include(std::slice::from_ref(&attachment))
            .map_err(|_| "Bedrock Chat 附件超出总数量或总大小限制")?;
        attachment_block(&attachment).map_err(|_| "无法构造 Bedrock 附件")
    }
}

fn image(value: &Value) -> Result<ImageAttachment, &'static str> {
    if !has_only(value, &["url", "detail"])
        || !(value["detail"].is_null() || value["detail"] == "auto")
    {
        return Err("Bedrock Chat 图片 detail 仅支持 auto");
    }
    let (mime, encoded) = value["url"]
        .as_str()
        .and_then(data_url)
        .ok_or("Bedrock Chat 图片须使用内联 Base64 data URL")?;
    decode_image(mime, encoded)
}

fn decode_image(mime: &str, encoded: &str) -> Result<ImageAttachment, &'static str> {
    if !matches!(mime, "image/png" | "image/jpeg" | "image/webp") {
        return Err("Bedrock Chat 图片格式须为 PNG、JPEG 或 WebP");
    }
    let bytes = decode(encoded, MAX_IMAGE_BYTES)?;
    let image = ImageAttachment::from_bytes(bytes).map_err(|_| "Bedrock Chat 图片无效")?;
    if image.mime() != mime
        || !encoded_dimensions(image.content()).is_some_and(|(width, height)| {
            (1..=8_000).contains(&width) && (1..=8_000).contains(&height)
        })
    {
        return Err("图片格式须与声明一致，且宽高须在 1 至 8000 像素之间");
    }
    Ok(image)
}

fn document(value: &Value) -> Result<DocumentAttachment, &'static str> {
    if !has_only(value, &["file_data", "filename", "file_id"]) || !value["file_id"].is_null() {
        return Err("Bedrock Chat 文档只接受内联文件");
    }
    let filename = value["filename"]
        .as_str()
        .ok_or("Bedrock Chat 文档需要 filename")?;
    let data = value["file_data"]
        .as_str()
        .ok_or("Bedrock Chat 文档需要 file_data")?;
    let (mime, encoded) = if data.starts_with("data:") {
        let (mime, encoded) = data_url(data).ok_or("Bedrock Chat 文档 data URL 无效")?;
        (Some(mime), encoded)
    } else {
        (None, data)
    };
    let bytes = decode(encoded, MAX_DOCUMENT_BYTES)?;
    let document = DocumentAttachment::from_bytes(filename, bytes)
        .map_err(|_| "Bedrock Chat 文档须为 PDF、UTF-8 文本或 Markdown")?;
    if mime.is_some_and(|mime| document.mime() != mime) {
        return Err("Bedrock Chat 文档格式须与声明一致");
    }
    Ok(document)
}

fn data_url(value: &str) -> Option<(&str, &str)> {
    let (header, data) = value.strip_prefix("data:")?.split_once(',')?;
    Some((header.strip_suffix(";base64")?, data))
}

fn decode(encoded: &str, limit: usize) -> Result<Vec<u8>, &'static str> {
    if encoded.len() > limit.div_ceil(3) * 4 {
        return Err("Bedrock Chat 附件超过单文件大小限制");
    }
    let bytes = Str::from_base64(encoded).map_err(|_| "Bedrock Chat 附件 Base64 无效")?;
    if bytes.len() > limit {
        return Err("Bedrock Chat 附件超过单文件大小限制");
    }
    Ok(bytes)
}
