#!/bin/sh
set -eu

output=$1
if [ -L .env.example ] || [ ! -f .env.example ] || [ -L artisan ] || [ ! -f artisan ]; then
    printf '%s\n' 'build_assets_invalid: Artisan 与环境示例须为普通文件' >&2
    exit 1
fi
install -m 755 artisan "$output/artisan"
install -d -m 755 "$output/storage" "$output/bootstrap/cache" "$output/modules"
cp -a .env.example public resources "$output/"
if [ -e lang ] || [ -L lang ]; then
    cp -a lang "$output/"
fi
for module in modules/*; do
    if [ -L "$module" ]; then
        printf '%s\n' 'build_assets_invalid: 模块目录不得为符号链接' >&2
        exit 1
    fi
    [ -d "$module" ] || continue
    destination="$output/$module"
    for asset in module.json public resources lang; do
        if [ -e "$module/$asset" ] || [ -L "$module/$asset" ]; then
            mkdir -p "$destination"
            cp -a "$module/$asset" "$destination/"
        fi
    done
done

# 公开资源和模板是发布输入。部署密钥、数据库、链接及特殊文件应留在包外。
cd "$output"
set -- public resources modules
if [ -e lang ] || [ -L lang ]; then set -- "$@" lang; fi
unsafe=$(find "$@" \( \( ! -type d ! -type f \) -o \
    \( -iname '.env' -o -iname '.env.*' -o -iname '*.key' -o -iname '*.p12' \
       -o -iname '*.pfx' -o -iname '*.sqlite*' -o -iname '*.db' \
       -o -iname '*.sql' -o -iname 'id_rsa' -o -iname 'id_ed25519' \) \) -print -quit)
if [ -n "$unsafe" ]; then
    printf '%s\n' 'build_assets_invalid: 发布资源含敏感名称、链接或特殊文件' >&2
    exit 1
fi
if grep -arqE -e '-{5}BEGIN ([A-Z0-9]+ )*PRIVATE KEY( BLOCK)?-{5}[[:space:]]*$' -- .env.example "$@"; then
    printf '%s\n' 'build_assets_invalid: 发布资源含私钥块' >&2
    exit 1
else
    status=$?
    [ "$status" -eq 1 ] || exit "$status"
fi
