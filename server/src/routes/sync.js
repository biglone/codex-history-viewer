import { query } from '../db/client.js';
import auth from '../middleware/auth.js';

export default async function syncRoutes(fastify) {

  // ─── 查询服务端状态 + 获取增量同步所需信息 ───────────────────────────────
  // POST /api/sync/check
  // Body: { device_id, since_ms, local_ids: [{id, updated_at_ms}] }
  // 返回: 需要上传的 ID（本地有但服务端没有或本地更新）、需要下载的 session（服务端有但本地没有）
  fastify.post('/sync/check', { preHandler: auth }, async (req, reply) => {
    const { device_id, since_ms = 0, local_ids = [] } = req.body || {};

    if (!device_id) {
      return reply.code(400).send({ error: 'Missing device_id' });
    }

    // 获取服务端所有 session 的 id + updated_at_ms
    const serverResult = await query(
      `SELECT id, updated_at_ms FROM sessions ORDER BY updated_at_ms DESC`
    );
    const serverMap = new Map(serverResult.rows.map(r => [r.id, Number(r.updated_at_ms)]));

    // 构建本地 Map
    const localMap = new Map(local_ids.map(r => [r.id, Number(r.updated_at_ms)]));

    // 计算差异
    const toUpload = [];    // 本地有、服务端没有，或本地更新时间更新
    const toDownload = [];  // 服务端有、本地没有，或服务端更新时间更新

    for (const [id, localUpdated] of localMap) {
      if (!serverMap.has(id) || localUpdated > serverMap.get(id)) {
        toUpload.push(id);
      }
    }

    for (const [id, serverUpdated] of serverMap) {
      if (!localMap.has(id) || serverUpdated > localMap.get(id)) {
        toDownload.push(id);
      }
    }

    return {
      to_upload: toUpload,        // 客户端需要上传这些 ID
      to_download: toDownload,    // 客户端需要下载这些 ID
      server_total: serverMap.size,
      local_total: localMap.size,
    };
  });


  // ─── 批量获取服务端 session 详情（用于下载同步） ─────────────────────────
  // POST /api/sync/pull
  // Body: { ids: [session_id...] }
  fastify.post('/sync/pull', { preHandler: auth }, async (req, reply) => {
    const { ids } = req.body || {};

    if (!Array.isArray(ids) || ids.length === 0) {
      return reply.code(400).send({ error: 'ids must be a non-empty array' });
    }
    if (ids.length > 100) {
      return reply.code(400).send({ error: 'Max 100 ids per pull request' });
    }

    // 批量查询（用 IN 子句）
    const placeholders = ids.map((_, i) => `$${i + 1}`).join(',');
    const result = await query(
      `SELECT id, device_id, device_name, title, cwd, model, model_provider,
              first_user_message, preview, created_at_ms, updated_at_ms,
              archived, message_count, messages_path
       FROM sessions WHERE id IN (${placeholders})`,
      ids
    );

    return {
      // node-postgres 默认将 BIGINT 列返回为字符串（防止 JS 精度丢失），
      // 但客户端 Rust 侧期望 i64 数字，必须显式转换。
      sessions: result.rows.map(r => ({
        ...r,
        created_at_ms:  r.created_at_ms  != null ? Number(r.created_at_ms)  : null,
        updated_at_ms:  r.updated_at_ms  != null ? Number(r.updated_at_ms)  : null,
        message_count:  r.message_count  != null ? Number(r.message_count)  : null,
        messages_path: undefined, // 不暴露内部路径
        has_messages: !!r.messages_path,
      })),
    };
  });


  // ─── 获取同步统计信息 ─────────────────────────────────────────────────────
  // GET /api/sync/stats
  fastify.get('/sync/stats', { preHandler: auth }, async (req, reply) => {
    const [totalRes, deviceRes, recentRes] = await Promise.all([
      query('SELECT COUNT(*) as total FROM sessions'),
      query('SELECT COUNT(*) as total FROM devices'),
      query(
        `SELECT device_id, device_name, COUNT(*) as count, MAX(synced_at) as last_sync
         FROM sessions GROUP BY device_id, device_name ORDER BY last_sync DESC`
      ),
    ]);

    return {
      total_sessions: parseInt(totalRes.rows[0].total),
      total_devices: parseInt(deviceRes.rows[0].total),
      devices: recentRes.rows.map(r => ({
        device_id: r.device_id,
        device_name: r.device_name,
        session_count: parseInt(r.count),
        last_sync: r.last_sync,
      })),
    };
  });


  // ─── 健康检查（不需要鉴权） ─────────────────────────────────────────────
  // GET /api/sync/health
  fastify.get('/sync/health', async (req, reply) => {
    try {
      await query('SELECT 1');
      return { ok: true, timestamp: new Date().toISOString() };
    } catch (err) {
      return reply.code(503).send({ ok: false, error: err.message });
    }
  });

}
