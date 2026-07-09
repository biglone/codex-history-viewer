import { query } from '../db/client.js';
import auth from '../middleware/auth.js';

export default async function automationRoutes(fastify) {

  // ─── 上传/更新 automations（批量） ────────────────────────────────────────
  // POST /api/automations/upload
  // Body: { device_id, device_name, automations: [{name, content, updated_at_ms}] }
  fastify.post('/automations/upload', { preHandler: auth }, async (req, reply) => {
    const { device_id, device_name = '', automations } = req.body || {};

    if (!device_id) {
      return reply.code(400).send({ error: 'Missing device_id' });
    }
    if (!Array.isArray(automations) || automations.length === 0) {
      return reply.code(400).send({ error: 'automations must be a non-empty array' });
    }
    if (automations.length > 200) {
      return reply.code(400).send({ error: 'Max 200 automations per batch' });
    }

    const results = [];

    for (const auto of automations) {
      if (!auto.name || typeof auto.content !== 'string') {
        results.push({ name: auto.name, ok: false, error: 'Missing name or content' });
        continue;
      }

      try {
        await query(
          `INSERT INTO automations (device_id, device_name, name, content, updated_at_ms, synced_at)
           VALUES ($1, $2, $3, $4, $5, now())
           ON CONFLICT (device_id, name) DO UPDATE SET
             device_name   = EXCLUDED.device_name,
             content       = EXCLUDED.content,
             updated_at_ms = EXCLUDED.updated_at_ms,
             synced_at     = now()
           WHERE EXCLUDED.updated_at_ms >= automations.updated_at_ms`,
          [device_id, device_name, auto.name, auto.content, auto.updated_at_ms || 0]
        );
        results.push({ name: auto.name, ok: true });
      } catch (err) {
        results.push({ name: auto.name, ok: false, error: err.message });
      }
    }

    return {
      ok: true,
      total: automations.length,
      uploaded: results.filter(r => r.ok).length,
      failed: results.filter(r => !r.ok).length,
      results,
    };
  });


  // ─── 检查差异：对比本地 automations 与服务端差异 ────────────────────────────
  // POST /api/automations/check
  // Body: { device_id, local_files: [{name, updated_at_ms}] }
  // 返回：需要上传的文件名、服务端其他设备有但本地没有或更旧的文件信息
  fastify.post('/automations/check', { preHandler: auth }, async (req, reply) => {
    const { device_id, local_files = [] } = req.body || {};

    if (!device_id) {
      return reply.code(400).send({ error: 'Missing device_id' });
    }

    // 获取该设备在服务端的 automations 列表
    const ownResult = await query(
      `SELECT name, updated_at_ms FROM automations WHERE device_id = $1`,
      [device_id]
    );
    const serverOwnMap = new Map(
      ownResult.rows.map(r => [r.name, Number(r.updated_at_ms)])
    );

    // 获取其他设备最新版本的 automations（取每个文件名最新的一条）
    const othersResult = await query(
      `SELECT DISTINCT ON (name) name, device_id, device_name, content, updated_at_ms
       FROM automations
       WHERE device_id != $1
       ORDER BY name, updated_at_ms DESC`,
      [device_id]
    );
    const othersMap = new Map(
      othersResult.rows.map(r => [r.name, {
        name: r.name,
        device_id: r.device_id,
        device_name: r.device_name,
        updated_at_ms: Number(r.updated_at_ms),
      }])
    );

    const localMap = new Map(
      local_files.map(f => [f.name, Number(f.updated_at_ms)])
    );

    // 计算需要上传的文件（本地有，服务端没有或本地更新）
    const toUpload = [];
    for (const [name, localMs] of localMap) {
      if (!serverOwnMap.has(name) || localMs > serverOwnMap.get(name)) {
        toUpload.push(name);
      }
    }

    // 计算可从其他设备拉取的文件（其他设备有，本地没有或本地更旧）
    const toDownload = [];
    for (const [name, info] of othersMap) {
      const localMs = localMap.get(name) || 0;
      if (info.updated_at_ms > localMs) {
        toDownload.push(info);
      }
    }

    return {
      to_upload: toUpload,
      to_download: toDownload,
      server_own_total: serverOwnMap.size,
      local_total: localMap.size,
    };
  });


  // ─── 批量拉取其他设备的 automation 文件内容 ──────────────────────────────────
  // POST /api/automations/pull
  // Body: { files: [{name, device_id}] }
  // 返回：带完整 content 的 automation 列表
  fastify.post('/automations/pull', { preHandler: auth }, async (req, reply) => {
    const { files } = req.body || {};

    if (!Array.isArray(files) || files.length === 0) {
      return reply.code(400).send({ error: 'files must be a non-empty array' });
    }
    if (files.length > 200) {
      return reply.code(400).send({ error: 'Max 200 files per pull request' });
    }

    const results = [];
    for (const f of files) {
      try {
        const res = await query(
          `SELECT name, device_id, device_name, content, updated_at_ms
           FROM automations
           WHERE device_id = $1 AND name = $2
           LIMIT 1`,
          [f.device_id, f.name]
        );
        if (res.rows.length > 0) {
          const row = res.rows[0];
          results.push({
            name: row.name,
            device_id: row.device_id,
            device_name: row.device_name,
            content: row.content,
            updated_at_ms: Number(row.updated_at_ms),
          });
        }
      } catch (err) {
        // 单文件查询失败不阻断整批，记录但跳过
        fastify.log.warn(`[automations/pull] 获取 ${f.device_id}/${f.name} 失败: ${err.message}`);
      }
    }

    return { automations: results };
  });


  // ─── 查询服务端 automations 统计 ────────────────────────────────────────────
  // GET /api/automations/stats
  fastify.get('/automations/stats', { preHandler: auth }, async (req, reply) => {
    const [totalRes, deviceRes] = await Promise.all([
      query('SELECT COUNT(*) as total FROM automations'),
      query(
        `SELECT device_id, device_name, COUNT(*) as count, MAX(synced_at) as last_sync
         FROM automations
         GROUP BY device_id, device_name
         ORDER BY last_sync DESC`
      ),
    ]);

    return {
      total_automations: parseInt(totalRes.rows[0].total),
      devices: deviceRes.rows.map(r => ({
        device_id: r.device_id,
        device_name: r.device_name,
        automation_count: parseInt(r.count),
        last_sync: r.last_sync,
      })),
    };
  });

}
