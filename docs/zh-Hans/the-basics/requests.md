# HTTP 请求

## 获取输入

用 `request.query(Some("key"))` 读取精确的顶层查询键。用 `request.input(Some("team.name")).await` 读取嵌套输入，传入 `None` 可读取整组输入。JSON 和表单的顶层字段优先于查询参数。只读取 JSON 正文时使用 `request.json`。

OpenRust 成功读取正文后保留原始字节，供后续读取。默认上限为 8 MiB。输入无效返回 400，超量返回 413。

## 文件

用 `request.file("attachments.0").await` 读取 multipart 上传文件，用 `request.hasFile("attachments").await` 检查字段是否包含文件。上传对象提供文件字节以及客户端给出的文件名和媒体类型。保存前应检查文件内容。

输入读取和 JSON 响应的完整示例见[控制器](./controllers.md)。
