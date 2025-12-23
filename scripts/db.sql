-- Create database if not exists (for manual initialization)
-- Note: Docker entrypoint will create database via POSTGRES_DB env var
SELECT 'CREATE DATABASE "GPUFabric"'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'GPUFabric')\gexec

-- Connect to GPUFabric database
\c GPUFabric

-- Create tokens table for authentication (only used fields)
CREATE TABLE IF NOT EXISTS "public"."tokens" (
    "id" BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    "user_id" BIGINT NOT NULL,
    "key" CHAR(48) UNIQUE NOT NULL,
    "status" BIGINT DEFAULT 1,
    "expired_time" BIGINT DEFAULT -1,  -- -1: never expires, otherwise unix timestamp
    "deleted_at" TIMESTAMP WITH TIME ZONE,
    "access_level" INTEGER DEFAULT 1  -- -1: all devices, 1: user's devices only
);

CREATE INDEX IF NOT EXISTS idx_tokens_key ON "public"."tokens" ("key");
CREATE INDEX IF NOT EXISTS idx_tokens_user_id ON "public"."tokens" ("user_id");
CREATE INDEX IF NOT EXISTS idx_tokens_deleted_at ON "public"."tokens" ("deleted_at");

-- Create GPU assets table for client info

CREATE TABLE  IF NOT EXISTS  "public"."gpu_assets" (
    "user_id" VARCHAR,
    "client_id" BYTEA PRIMARY KEY,
    "client_name" VARCHAR,
    "client_status" VARCHAR DEFAULT 'active',
    "valid_status" VARCHAR DEFAULT 'valid',
    "os_type" VARCHAR,
    "outo_set_model" BOOLEAN DEFAULT TRUE,
    "model" VARCHAR,
    "model_version" VARCHAR,
    "model_version_code" BIGINT,
    "created_at" TIMESTAMP DEFAULT NOW(),
    "updated_at" TIMESTAMP DEFAULT NOW()
);

 CREATE INDEX IF NOT EXISTS idx_gpu_assets_user_id_client_name
 ON "public"."gpu_assets" ("user_id", "client_name");

CREATE TABLE IF NOT EXISTS "public"."pod_info" (
    "pod_id" UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    "client_id" BYTEA NOT NULL REFERENCES "public"."gpu_assets" ("client_id") ON DELETE CASCADE,
    "pod_name" VARCHAR(255) NOT NULL,
    "node_name" VARCHAR(255),
    "pod_type" VARCHAR(64) DEFAULT 'compute',
    "device_count" SMALLINT DEFAULT 0,
    "total_memory_mb" INTEGER DEFAULT 0,
    "total_power_w" INTEGER DEFAULT 0,
    "auto_set_model" BOOLEAN DEFAULT TRUE,
    "model" VARCHAR(255),
    "model_version" VARCHAR(255),
    "model_version_code" BIGINT,
    "created_at" TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    "updated_at" TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(client_id, pod_name)
);

CREATE INDEX IF NOT EXISTS idx_pod_client_id ON "public"."pod_info" ("client_id");

CREATE TABLE  IF NOT EXISTS  "public"."system_info" (
    client_id BYTEA PRIMARY KEY,
    cpu_usage   SMALLINT,
    mem_usage   SMALLINT,
    disk_usage  SMALLINT,
    device_memsize BIGINT,
    device_count INTEGER DEFAULT 1,
    total_tflops INTEGER DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE  IF NOT EXISTS  "public"."device_info" (
    client_id BYTEA NOT NULL,
    device_index SMALLINT,
    device_name VARCHAR(255) DEFAULT NULL,
    device_id INTEGER,
    vendor_id INTEGER,
    device_memusage SMALLINT,
    device_gpuusage SMALLINT,
    device_powerusage SMALLINT,
    device_temp SMALLINT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (client_id, device_index)
);

CREATE TABLE  IF NOT EXISTS  client_models (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,      
    version VARCHAR(50) NOT NULL,    
    version_code BIGINT NOT NULL,    
    is_active BOOLEAN DEFAULT TRUE,  
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    engine_type SMALLINT NOT NULL DEFAULT 1,
    min_memory_mb INTEGER,           
    min_gpu_memory_gb INTEGER,       
    UNIQUE(name, version),
    UNIQUE(name, version_code),
    CONSTRAINT version_code_check CHECK (version_code > 0)
);

CREATE TABLE IF NOT EXISTS heartbeat (
  id SERIAL,
  client_id   BYTEA NOT NULL,
  cpu_usage   SMALLINT,
  mem_usage   SMALLINT,
  disk_usage  SMALLINT,
  network_up BIGINT NOT NULL DEFAULT 0,
  network_down BIGINT NOT NULL DEFAULT 0,
  timestamp   TIMESTAMPTZ NOT NULL,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (client_id, timestamp)
);

CREATE INDEX IF NOT EXISTS idx_heartbeat_client_id_timestamp 
ON heartbeat (client_id, timestamp DESC);

CREATE TABLE IF NOT EXISTS client_daily_stats (
    id BIGSERIAL PRIMARY KEY,
    date DATE NOT NULL,
    client_id BYTEA NOT NULL,
    total_heartbeats INTEGER NOT NULL DEFAULT 0,
    avg_cpu_usage FLOAT,
    avg_memory_usage FLOAT,
    avg_disk_usage FLOAT,
    total_network_in_bytes BIGINT DEFAULT 0,
    total_network_out_bytes BIGINT DEFAULT 0,
    last_heartbeat TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (client_id, date)
);

CREATE INDEX IF NOT EXISTS idx_client_daily_stats_client_id_date 
ON client_daily_stats (client_id, date DESC);

CREATE TABLE IF NOT EXISTS device_daily_stats (
    id BIGSERIAL,
    date DATE NOT NULL,
    client_id BYTEA NOT NULL,
    device_index SMALLINT NOT NULL,                
    device_name VARCHAR(255),            
    total_heartbeats INTEGER NOT NULL DEFAULT 0,
    avg_utilization FLOAT,
    avg_temperature FLOAT,
    avg_power_usage FLOAT,
    avg_memory_usage FLOAT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (client_id, device_index, date)
);

CREATE INDEX IF NOT EXISTS idx_device_daily_stats_date ON device_daily_stats (date);
CREATE INDEX IF NOT EXISTS idx_device_daily_stats_client_id ON device_daily_stats (client_id);
CREATE INDEX IF NOT EXISTS idx_device_daily_stats_device_index ON device_daily_stats (device_index);

-- Insert test data for client_models
INSERT INTO client_models (name, version, version_code, is_active, created_at, engine_type, min_memory_mb, min_gpu_memory_gb) 
VALUES 
    ('llama3.2:latest', 'latest', 12314, true, '2025-10-14 16:19:31.846481+08', 1, NULL, 8),
    ('TheBloke/gemma-3-12b-it-GPTQ', 'GPTQ', 12315, true, '2025-10-14 23:34:40.663107+08', 2, NULL, 8),
    ('facebook/opt-125m', 'latest', 12316, true, '2025-10-15 11:06:15.912991+08', 2, NULL, 8)
ON CONFLICT (name, version) DO NOTHING;

-- Insert test data for tokens
INSERT INTO tokens (user_id, key, status, expired_time, deleted_at, access_level) 
VALUES 
    (2, 'HSSb0OFrZon7wapKUduWqSxqpELMI62eTPyW017QanhnMyy4', 1, -1, NULL, 1)
ON CONFLICT (key) DO NOTHING;