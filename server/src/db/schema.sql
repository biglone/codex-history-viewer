-- Codex Sync Server 数据库建表语句
-- 执行: node src/db/init.js

-- 会话元数据表
CREATE TABLE IF NOT EXISTS sessions (
  id            TEXT PRIMARY KEY,          -- 原始 session_id（UUID）
  device_id     TEXT NOT NULL,             -- 来源设备标识
  device_name   TEXT,                      -- 来源设备名称（可读）
  title         TEXT,
  cwd           TEXT,
  model         TEXT,
  model_provider TEXT,
  first_user_message TEXT,
  preview       TEXT,
  created_at_ms BIGINT,
  updated_at_ms BIGINT,
  archived      BOOLEAN DEFAULT false,
  messages_path TEXT,                      -- 存储的 JSONL 文件相对路径
  synced_at     TIMESTAMPTZ DEFAULT now(), -- 最后同步时间
  message_count INT DEFAULT 0             -- 消息数量（冗余字段，方便显示）
);

-- 同步日志表（记录每次上传/下载操作，方便调试和增量同步）
CREATE TABLE IF NOT EXISTS sync_log (
  id          SERIAL PRIMARY KEY,
  device_id   TEXT NOT NULL,
  session_id  TEXT NOT NULL,
  action      TEXT NOT NULL CHECK (action IN ('upload', 'download', 'delete')),
  status      TEXT NOT NULL CHECK (status IN ('ok', 'error')),
  detail      TEXT,
  created_at  TIMESTAMPTZ DEFAULT now()
);

-- 设备表（记录已注册的设备信息）
CREATE TABLE IF NOT EXISTS devices (
  id          TEXT PRIMARY KEY,            -- 设备唯一 ID（客户端自生成 UUID）
  name        TEXT,
  platform    TEXT,                        -- 'mac' | 'windows' | 'linux'
  last_seen   TIMESTAMPTZ DEFAULT now(),
  first_seen  TIMESTAMPTZ DEFAULT now()
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_sessions_device ON sessions(device_id);
CREATE INDEX IF NOT EXISTS idx_sessions_updated ON sessions(updated_at_ms DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_created ON sessions(created_at_ms DESC);
CREATE INDEX IF NOT EXISTS idx_sync_log_device ON sync_log(device_id, created_at DESC);

-- 全文搜索索引（可选，PostgreSQL 内置）
CREATE INDEX IF NOT EXISTS idx_sessions_fts ON sessions
  USING gin(to_tsvector('simple', coalesce(title,'') || ' ' || coalesce(first_user_message,'') || ' ' || coalesce(preview,'')));

-- ───────────────────────────────────────────────────────────────────────
-- Automations 同步表
-- 存储每台设备上传的 ~/.codex/automations/ 目录中的文件
-- 以 (device_id, name) 作为业务唯一键，支持跨设备合并最新版本
-- ───────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS automations (
  id            SERIAL PRIMARY KEY,
  device_id     TEXT NOT NULL,             -- 来源设备 ID
  device_name   TEXT,                      -- 来源设备名称（可读）
  name          TEXT NOT NULL,             -- 文件名，如 "daily-report.json"
  content       TEXT NOT NULL,             -- 文件完整内容（UTF-8）
  updated_at_ms BIGINT NOT NULL,           -- 客户端文件最后修改时间（用于增量对比）
  synced_at     TIMESTAMPTZ DEFAULT now(), -- 服务端最后同步时间
  UNIQUE (device_id, name)                 -- 每设备每文件名唯一
);

CREATE INDEX IF NOT EXISTS idx_automations_device ON automations(device_id);
CREATE INDEX IF NOT EXISTS idx_automations_name ON automations(name);
CREATE INDEX IF NOT EXISTS idx_automations_updated ON automations(updated_at_ms DESC);
