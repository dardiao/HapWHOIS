import React, { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

function formatDate(iso) {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return (
    d.toLocaleString("zh-CN", {
      timeZone: "UTC",
      dateStyle: "medium",
      timeStyle: "short",
    }) + " UTC"
  );
}

function formatElapsed(seconds) {
  const m = String(Math.floor(seconds / 60)).padStart(2, "0");
  const s = String(seconds % 60).padStart(2, "0");
  return `${m}:${s}`;
}

function sourceLabel(item) {
  const rdap = !!item.rdap;
  const whois = !!item.whoisRaw;
  if (rdap && whois) return "RDAP+WHOIS";
  if (rdap) return "RDAP";
  if (whois) return "WHOIS";
  return "—";
}

function whoisServerLabel(server) {
  if (!server) return "—";
  return server.includes("whois-servers.net") ? `${server}（DNS）` : server;
}

function statusOf(item) {
  if (item.error?.includes("已停止")) return { text: "未执行", cls: "badge-gray" };
  if (item.error) return { text: "失败", cls: "badge-red" };
  return { text: "成功", cls: "badge-green" };
}

function ResultRow({ item }) {
  const status = statusOf(item);
  return (
    <div className="brow">
      <div className="brow-main">
        <div className="bcell bcell-domain">
          <span className="brow-name">{item.domain}</span>
          <span className={`badge ${status.cls}`}>{status.text}</span>
        </div>
        <div className="bcell">
          <span className="bcell-label">数据源</span>
          {status.text === "未执行" ? "—" : sourceLabel(item)}
        </div>
        <div className="bcell">
          <span className="bcell-label">注册商</span>
          {item.rdap?.registrar ?? (status.text === "未执行" ? "—" : "仅 WHOIS")}
        </div>
        <div className="bcell">
          <span className="bcell-label">到期时间</span>
          {formatDate(item.rdap?.expirationDate)}
        </div>
        <div className="bcell">
          <span className="bcell-label">WHOIS 服务器</span>
          {status.text === "未执行" ? "—" : whoisServerLabel(item.whoisServer)}
        </div>
        {item.error && !item.error.includes("已停止") && (
          <div className="bcell bcell-error" title={item.error}>
            {item.error}
          </div>
        )}
      </div>
      {(item.rdap || item.whoisRaw) && (
        <details className="brow-detail">
          <summary>详情 / 原始 WHOIS</summary>
          <div className="brow-detail-body">
            {item.rdap?.status?.length > 0 && (
              <div className="field">
                <span className="field-label">状态</span>
                <div className="chip-row">
                  {item.rdap.status.map((s) => (
                    <span key={s} className="chip">
                      {s}
                    </span>
                  ))}
                </div>
              </div>
            )}
            {item.rdap?.nameservers?.length > 0 && (
              <div className="field">
                <span className="field-label">Name Server</span>
                <div className="chip-row">
                  {item.rdap.nameservers.map((ns) => (
                    <span key={ns} className="chip chip-blue">
                      {ns}
                    </span>
                  ))}
                </div>
              </div>
            )}
            {item.rdap && (
              <div className="detail-grid">
                <span>注册时间：{formatDate(item.rdap.creationDate)}</span>
                <span>最近更新：{formatDate(item.rdap.updatedDate)}</span>
              </div>
            )}
            {item.whoisRaw && <pre>{item.whoisRaw}</pre>}
          </div>
        </details>
      )}
    </div>
  );
}

export default function App() {
  const [input, setInput] = useState("");
  const [phase, setPhase] = useState("idle"); // idle | loading | done | error
  const [items, setItems] = useState([]);
  const [progress, setProgress] = useState({ done: 0, total: 0 });
  const [elapsed, setElapsed] = useState(0);
  const [error, setError] = useState(null);
  const [useDnsDiscovery, setUseDnsDiscovery] = useState(true);

  const itemsRef = useRef([]);
  const pendingRef = useRef([]);
  const flushTimer = useRef(null);
  const elapsedTimer = useRef(null);
  const unlistenRef = useRef([]);

  const domains = input
    .split(/\n/)
    .map((s) => s.trim())
    .filter(Boolean);

  const flush = useCallback(() => {
    if (!pendingRef.current.length) return;
    const batch = pendingRef.current;
    pendingRef.current = [];
    setItems((prev) => [...prev, ...batch]);
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const un1 = await listen("lookup-start", (e) => {
        if (cancelled) return;
        setProgress({ done: 0, total: e.payload.total });
      });
      const un2 = await listen("lookup-progress", (e) => {
        if (cancelled) return;
        itemsRef.current.push(e.payload.item);
        pendingRef.current.push(e.payload.item);
        setProgress({ done: e.payload.done, total: e.payload.total });
        if (pendingRef.current.length >= 40) flush();
      });
      unlistenRef.current = [un1, un2];
    })();
    return () => {
      cancelled = true;
      unlistenRef.current.forEach((fn) => fn());
    };
  }, [flush]);

  const doLookup = useCallback(async () => {
    if (!domains.length || phase === "loading") return;
    itemsRef.current = [];
    pendingRef.current = [];
    setItems([]);
    setError(null);
    setElapsed(0);
    setProgress({ done: 0, total: domains.length });
    setPhase("loading");

    flushTimer.current = setInterval(flush, 200);
    elapsedTimer.current = setInterval(() => {
      setElapsed((s) => s + 1);
    }, 1000);

    try {
      const results = await invoke("lookup_batch", { domains, useDnsDiscovery });
      itemsRef.current = results;
      pendingRef.current = [];
      setItems(results);
      setPhase("done");
    } catch (e) {
      setError(String(e));
      setPhase("error");
    } finally {
      clearInterval(flushTimer.current);
      clearInterval(elapsedTimer.current);
    }
  }, [domains, phase, useDnsDiscovery, flush]);

  const doStop = useCallback(async () => {
    try {
      await invoke("cancel_lookup");
    } catch {
      // 忽略
    }
  }, []);

  const successCount = items.filter((i) => !i.error).length;
  const failedCount = items.filter((i) => i.error && !i.error.includes("已停止")).length;
  const stoppedCount = items.filter((i) => i.error?.includes("已停止")).length;

  return (
    <div className="app">
      <header className="topbar">
        <div className="logo">H</div>
        <div>
          <h1>HapWHOIS</h1>
          <p>域名批量信息查询 · 按后缀自动路由 RDAP / WHOIS 服务器</p>
        </div>
      </header>

      <main className="content">
        <form
          className="batch-form"
          onSubmit={(e) => {
            e.preventDefault();
            doLookup();
          }}
        >
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder={"每行一个域名，例如：\nexample.com\ngoogle.cn\ngithub.io"}
            rows={5}
            spellCheck={false}
            disabled={phase === "loading"}
          />
          <label className="dns-option">
            <input
              type="checkbox"
              checked={useDnsDiscovery}
              onChange={(e) => setUseDnsDiscovery(e.target.checked)}
              disabled={phase === "loading"}
            />
            内置表查不到后缀时，用 {`{后缀}`}.whois-servers.net 自动发现服务器
          </label>
          <div className="search-row">
            <p className="hint-left">
              已识别 <strong>{domains.length}</strong> 个域名 · 并发 6 · 单域名超时 10s
            </p>
            {phase === "loading" ? (
              <button type="button" className="btn-stop" onClick={doStop}>
                停止
              </button>
            ) : (
              <button type="submit" disabled={!domains.length}>
                批量查询
              </button>
            )}
          </div>
        </form>

        {phase === "loading" && (
          <div className="progress-bar-wrap">
            <div
              className="progress-bar"
              style={{
                width: `${progress.total ? Math.round((progress.done / progress.total) * 100) : 0}%`,
              }}
            />
          </div>
        )}

        {phase === "loading" && (
          <div className="progress-text">
            正在查询 {progress.done} / {progress.total || domains.length} · 已用时{" "}
            {formatElapsed(elapsed)} · 结果会实时滚出，可随时停止
          </div>
        )}

        {phase === "error" && <div className="error-box">{error}</div>}

        {items.length > 0 && (
          <div className="result-stack">
            <div className="summary">
              共 {items.length} 个：
              <span className="summary-ok">{successCount} 成功</span>
              {failedCount > 0 && <span className="summary-fail">{failedCount} 失败</span>}
              {stoppedCount > 0 && <span className="summary-stop">{stoppedCount} 未执行</span>}
              {phase === "done" && !stoppedCount && "（完成）"}
              {phase === "done" && stoppedCount > 0 && "（已停止）"}
            </div>
            <div className="batch-table">
              <div className="brow brow-head">
                <div className="bcell bcell-domain">域名</div>
                <div className="bcell">数据源</div>
                <div className="bcell">注册商</div>
                <div className="bcell">到期时间</div>
                <div className="bcell">WHOIS 服务器</div>
              </div>
              {items.map((item) => (
                <ResultRow key={item.domain} item={item} />
              ))}
            </div>
          </div>
        )}

        {phase === "idle" && (
          <p className="hint">
            按域名后缀自动路由：.com/.net → Verisign，.cn → CNNIC，.io → Identity Digital……
            RDAP 优先，传统 WHOIS 兜底
          </p>
        )}
      </main>
    </div>
  );
}

