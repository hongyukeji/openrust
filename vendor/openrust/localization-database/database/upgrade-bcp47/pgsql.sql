-- 一次性迁移模板：先运行对应 inventory，备份并审阅表名、映射及冲突。
-- 默认最终 ROLLBACK；正式执行须明确改为 COMMIT。不要使用客户端忽略错误选项。
-- 仅 language_lines.text 的语言键；不改 group/key，不作为运行时兼容层。
BEGIN;
LOCK TABLE language_lines IN SHARE ROW EXCLUSIVE MODE;
CREATE TEMPORARY TABLE openrust_locale_guard (valid INTEGER NOT NULL CHECK (valid = 1));
-- 冲突或未知旧键会触发约束失败，必须回滚整个事务，禁止覆盖两份内容。
INSERT INTO openrust_locale_guard SELECT CASE WHEN EXISTS (SELECT 1 FROM language_lines WHERE ((("text"::jsonb ? 'zh_CN') AND ("text"::jsonb ? 'zh-Hans')) OR (("text"::jsonb ? 'zh_TW') AND ("text"::jsonb ? 'zh-Hant')) OR (("text"::jsonb ? 'en_US') AND ("text"::jsonb ? 'en-US')) OR (("text"::jsonb ? 'en_GB') AND ("text"::jsonb ? 'en-GB'))) OR (EXISTS (SELECT 1 FROM jsonb_object_keys("text"::jsonb) AS tags(tag) WHERE strpos(tag, '_') > 0 AND tag NOT IN ('zh_CN', 'zh_TW', 'en_US', 'en_GB')))) THEN 0 ELSE 1 END;
UPDATE language_lines SET "text" = (jsonb_populate_record(NULL::language_lines, jsonb_build_object('text', (("text"::jsonb - ARRAY['zh_CN', 'zh_TW', 'en_US', 'en_GB']) || CASE WHEN ("text"::jsonb ? 'zh_CN') THEN jsonb_build_object('zh-Hans', "text"::jsonb -> 'zh_CN') ELSE '{}'::jsonb END || CASE WHEN ("text"::jsonb ? 'zh_TW') THEN jsonb_build_object('zh-Hant', "text"::jsonb -> 'zh_TW') ELSE '{}'::jsonb END || CASE WHEN ("text"::jsonb ? 'en_US') THEN jsonb_build_object('en-US', "text"::jsonb -> 'en_US') ELSE '{}'::jsonb END || CASE WHEN ("text"::jsonb ? 'en_GB') THEN jsonb_build_object('en-GB', "text"::jsonb -> 'en_GB') ELSE '{}'::jsonb END))))."text"
WHERE ("text"::jsonb ? 'zh_CN') OR ("text"::jsonb ? 'zh_TW') OR ("text"::jsonb ? 'en_US') OR ("text"::jsonb ? 'en_GB');
DROP TABLE openrust_locale_guard;
ROLLBACK;
