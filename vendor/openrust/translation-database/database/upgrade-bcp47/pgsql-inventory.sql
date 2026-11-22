-- 只读盘点：所有映射冲突及未列入映射的旧下划线语言键。
SELECT * FROM language_lines WHERE ((("text"::jsonb ? 'zh_CN') AND ("text"::jsonb ? 'zh-Hans')) OR (("text"::jsonb ? 'zh_TW') AND ("text"::jsonb ? 'zh-Hant')) OR (("text"::jsonb ? 'en_US') AND ("text"::jsonb ? 'en-US')) OR (("text"::jsonb ? 'en_GB') AND ("text"::jsonb ? 'en-GB'))) OR (EXISTS (SELECT 1 FROM jsonb_object_keys("text"::jsonb) AS tags(tag) WHERE strpos(tag, '_') > 0 AND tag NOT IN ('zh_CN', 'zh_TW', 'en_US', 'en_GB')));
-- 待迁移行；仅查看，不修改。
SELECT * FROM language_lines WHERE ("text"::jsonb ? 'zh_CN') OR ("text"::jsonb ? 'zh_TW') OR ("text"::jsonb ? 'en_US') OR ("text"::jsonb ? 'en_GB');
