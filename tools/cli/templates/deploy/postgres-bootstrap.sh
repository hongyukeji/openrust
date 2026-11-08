# 官方镜像仅在初始化空数据目录时读取此脚本。
psql --no-psqlrc --set ON_ERROR_STOP=1 --single-transaction --file - \
  --username "$POSTGRES_USER" --dbname postgres \
  --set app_user="$OPENRUST_DB_USER" --set app_password="$OPENRUST_DB_PASSWORD" --set app_db="$POSTGRES_DB" <<'SQL'
CREATE ROLE :"app_user" LOGIN PASSWORD :'app_password';
ALTER DATABASE :"app_db" OWNER TO :"app_user";
SQL
