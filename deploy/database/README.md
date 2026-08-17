# 本地数据库测试配方

用于 dby 驱动集成测试的本地 MySQL 实例（MySQL 5.7 与 8.0）。

```bash
# 启动（MySQL 8.0 监听 127.0.0.1:33061，5.7 监听 33062）
docker compose up -d mysql80

# 运行集成测试（默认连 127.0.0.1:3306，可通过环境变量覆盖）
DBY_TEST_MYSQL_PORT=33061 DBY_TEST_MYSQL_PASSWORD=dby-test \
  cargo test -p dby-driver-mysql --test mysql_integration -- --ignored --nocapture
```

环境变量（均有默认值）：

| 变量 | 默认 |
| --- | --- |
| `DBY_TEST_MYSQL_HOST` | `127.0.0.1` |
| `DBY_TEST_MYSQL_PORT` | `3306` |
| `DBY_TEST_MYSQL_USER` | `root` |
| `DBY_TEST_MYSQL_PASSWORD` | `dby-test` |
| `DBY_TEST_MYSQL_DB` | `dby_test` |
