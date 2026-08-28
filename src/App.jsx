import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

const LETTERS = "abcdefghijklmnopqrstuvwxyz";
const DIGITS = "0123456789";

const PATTERNS = {
  1: [
    { id: "a", label: "纯字母（a）", count: 26 },
    { id: "0", label: "纯数字（0）", count: 10 },
  ],
  2: [
    { id: "aa", label: "纯字母（aa）", count: 676 },
    { id: "0a", label: "1字母1数字·数字第1位（0a）", count: 260 },
    { id: "a0", label: "1字母1数字·数字第2位（a0）", count: 260 },
    { id: "00", label: "纯数字（00）", count: 100 },
  ],
  3: [
    { id: "aaa", label: "纯字母（aaa）", count: 17576 },
    { id: "0aa", label: "2字母1数字·数字第1位（0aa）", count: 6760 },
    { id: "a0a", label: "2字母1数字·数字第2位（a0a）", count: 6760 },
    { id: "aa0", label: "2字母1数字·数字第3位（aa0）", count: 6760 },
    { id: "a00", label: "1字母2数字·字母第1位（a00）", count: 2600 },
    { id: "0a0", label: "1字母2数字·字母第2位（0a0）", count: 2600 },
    { id: "00a", label: "1字母2数字·字母第3位（00a）", count: 2600 },
    { id: "000", label: "纯数字（000）", count: 1000 },
  ],
};

const WORD_MODES = [
  { id: "alone", label: "关键词本身（cloud）" },
  { id: "kw+af", label: "关键词+搭配词（cloudweb）" },
  { id: "af+kw", label: "搭配词+关键词（webcloud）" },
  { id: "kw-af", label: "关键词-搭配词（cloud-web）" },
  { id: "af-kw", label: "搭配词-关键词（web-cloud）" },
  { id: "kw+num", label: "关键词+数字（cloud01）" },
  { id: "num+kw", label: "数字+关键词（01cloud）" },
];

const DEFAULT_KEYWORDS = "cloud, star, nova, peak, pixel, echo, orbit";
const DEFAULT_AFFIXES = "web, net, app, hub, lab, tech, pro, ai, box, x, io, 01, 2, 3, 5, 7, 9";

function parseList(text) {
  return text
    .split(/[\n,;，；\s]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

function cartesian(pattern) {
  const result = [];
  const alphabet = (ch) => (ch === "a" ? LETTERS : DIGITS);
  const build = (prefix, i) => {
    if (i === pattern.length) {
      result.push(prefix);
      return;
    }
    for (const ch of alphabet(pattern[i])) build(prefix + ch, i + 1);
  };
  build("", 0);
  return result;
}

function generateLetters(minLen, maxLen, checked, cap) {
  const all = [];
  for (let len = minLen; len <= maxLen; len++) {
    for (const p of PATTERNS[len]) {
      if (checked.has(p.id)) all.push(...cartesian(p.id));
    }
  }
  const seen = new Set();
  const res = [];
  for (const s of all) {
    if (!seen.has(s)) {
      seen.add(s);
      res.push(s);
    }
  }
  res.sort();
  return res.slice(0, cap);
}

function generateWords(keywords, affixes, modes) {
  const out = new Set();
  const nums = affixes.filter((a) => /^\d+$/.test(a));
  const numberPool = nums.length ? nums : DIGITS.split("");
  for (const k of keywords) {
    if (modes.has("alone")) out.add(k);
    for (const a of affixes) {
      if (modes.has("kw+af")) out.add(k + a);
      if (modes.has("af+kw")) out.add(a + k);
      if (modes.has("kw-af")) out.add(k + "-" + a);
      if (modes.has("af-kw")) out.add(a + "-" + k);
    }
    for (const n of numberPool) {
      if (modes.has("kw+num")) out.add(k + n);
      if (modes.has("num+kw")) out.add(n + k);
    }
  }
  return [...out];
}

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
  const [tab, setTab] = useState("query"); // query | dict

  // ---- 批量查询 ----
  const [input, setInput] = useState("");
  const [phase, setPhase] = useState("idle");
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

  // ---- 字典生成 ----
  const [dict, setDict] = useState([]);
  const [manualText, setManualText] = useState("");
  const [keywords, setKeywords] = useState(DEFAULT_KEYWORDS);
  const [affixes, setAffixes] = useState(DEFAULT_AFFIXES);
  const [wordModes, setWordModes] = useState(["alone", "kw+af", "af+kw"]);
  const [minLen, setMinLen] = useState(1);
  const [maxLen, setMaxLen] = useState(3);
  const [letterTypes, setLetterTypes] = useState(["a", "0", "aa", "0a", "a0", "00"]);
  const [cap, setCap] = useState(500);
  const [dictMsg, setDictMsg] = useState("");

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

  // ---- 字典操作 ----
  const appendEntries = useCallback(
    (entries) => {
      const set = new Set(dict);
      const fresh = [];
      for (const e of entries) {
        if (!set.has(e)) {
          set.add(e);
          fresh.push(e);
        }
      }
      if (fresh.length) setDict([...set]);
      return fresh.length;
    },
    [dict],
  );

  const letterEstimate = useMemo(() => {
    let total = 0;
    for (let len = minLen; len <= maxLen; len++) {
      for (const p of PATTERNS[len]) {
        if (letterTypes.includes(p.id)) total += p.count;
      }
    }
    return Math.min(cap, total);
  }, [minLen, maxLen, letterTypes, cap]);

  const generateWord = () => {
    const modes = new Set(wordModes);
    if (!modes.size) {
      setDictMsg("请至少勾选一种组合方式");
      return;
    }
    const items = generateWords(parseList(keywords), parseList(affixes), modes);
    const added = appendEntries(items);
    setDictMsg(`词根组合生成 ${items.length} 个，新增 ${added} 个`);
  };

  const generateLetter = () => {
    const checked = new Set(letterTypes);
    if (!checked.size) {
      setDictMsg("请至少勾选一种组合类型");
      return;
    }
    if (minLen > maxLen) {
      setDictMsg("最短长度不能大于最长长度");
      return;
    }
    const items = generateLetters(minLen, maxLen, checked, cap);
    const added = appendEntries(items);
    setDictMsg(`字母组合生成 ${items.length} 个，新增 ${added} 个`);
  };

  const addManual = () => {
    const items = parseList(manualText);
    if (!items.length) {
      setDictMsg("手动输入为空");
      return;
    }
    const added = appendEntries(items);
    setDictMsg(`手动输入新增 ${added} 个`);
  };

  const importFile = async () => {
    const path = await open({
      multiple: false,
      filters: [{ name: "文本", extensions: ["txt", "list", "csv"] }],
    });
    if (!path) return;
    const content = await invoke("read_dict_file", { path });
    setManualText(content);
    const added = appendEntries(parseList(content));
    setDictMsg(`已导入 ${path}，新增 ${added} 个`);
  };

  const exportDict = async () => {
    if (!dict.length) {
      setDictMsg("字典为空，先生成或导入一些条目");
      return;
    }
    const path = await save({
      defaultPath: "hapwhois-dictionary.txt",
      filters: [{ name: "文本", extensions: ["txt"] }],
    });
    if (!path) return;
    await invoke("write_dict_file", { path, content: dict.join("\n") + "\n" });
    setDictMsg(`已导出 ${dict.length} 条 → ${path}`);
  };

  const copyDict = async () => {
    if (!dict.length) {
      setDictMsg("字典为空");
      return;
    }
    try {
      await navigator.clipboard.writeText(dict.join("\n"));
    } catch {
      const ta = document.createElement("textarea");
      ta.value = dict.join("\n");
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      document.body.removeChild(ta);
    }
    setDictMsg(`已复制 ${dict.length} 条`);
  };

  const importDictToQuery = () => {
    if (!dict.length) return;
    const set = new Set(parseList(input));
    const merged = [...set, ...dict.filter((d) => !set.has(d))];
    setInput(merged.join("\n"));
    setTab("query");
    setDictMsg(`已导入 ${dict.length} 条到查询列表`);
  };

  const toggleWordMode = (id) => {
    setWordModes((prev) => (prev.includes(id) ? prev.filter((m) => m !== id) : [...prev, id]));
  };

  const toggleLetterType = (id) => {
    setLetterTypes((prev) =>
      prev.includes(id) ? prev.filter((t) => t !== id) : [...prev, id],
    );
  };

  const successCount = items.filter((i) => !i.error).length;
  const failedCount = items.filter((i) => i.error && !i.error.includes("已停止")).length;
  const stoppedCount = items.filter((i) => i.error?.includes("已停止")).length;

  return (
    <div className="app">
      <header className="topbar">
        <div className="logo">H</div>
        <div>
          <h1>HapWHOIS</h1>
          <p>域名批量信息查询 · 内置字典生成器</p>
        </div>
      </header>

      <nav className="tabs">
        <button className={tab === "query" ? "tab active" : "tab"} onClick={() => setTab("query")}>
          批量查询
        </button>
        <button className={tab === "dict" ? "tab active" : "tab"} onClick={() => setTab("dict")}>
          字典生成{dict.length ? `（${dict.length}）` : ""}
        </button>
      </nav>

      {tab === "query" && (
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
                {dict.length > 0 && (
                  <button type="button" className="link-btn" onClick={importDictToQuery}>
                    从字典导入（{dict.length}）
                  </button>
                )}
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
                <div className="brow brow-head brow-main">
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
      )}

      {tab === "dict" && (
        <main className="content dict-content">
          <div className="dict-grid">
            <section className="dict-section">
              <h3>手动输入 / 导入</h3>
              <p className="desc">粘贴或导入已有的域名/字典，逐行或逗号分隔</p>
              <textarea
                className="mini"
                value={manualText}
                onChange={(e) => setManualText(e.target.value)}
                placeholder={"手动输入域名或字典，每行一个"}
                rows={4}
                spellCheck={false}
              />
              <div className="dict-actions">
                <button type="button" className="btn-small" onClick={addManual}>
                  加入列表
                </button>
                <button type="button" className="btn-small" onClick={importFile}>
                  导入文件…
                </button>
              </div>
            </section>

            <section className="dict-section">
              <h3>词根组合（起名助手）</h3>
              <p className="desc">围绕你喜欢的词，自动拼出各种组合</p>
              <label className="mini-label">关键词（想围绕什么词起名）</label>
              <textarea
                className="mini"
                value={keywords}
                onChange={(e) => setKeywords(e.target.value)}
                rows={3}
                spellCheck={false}
              />
              <label className="mini-label">搭配词（自动加到关键词前后）</label>
              <textarea
                className="mini"
                value={affixes}
                onChange={(e) => setAffixes(e.target.value)}
                rows={3}
                spellCheck={false}
              />
              <label className="mini-label">组合方式</label>
              <div className="checkbox-grid">
                {WORD_MODES.map((m) => (
                  <label key={m.id}>
                    <input
                      type="checkbox"
                      checked={wordModes.includes(m.id)}
                      onChange={() => toggleWordMode(m.id)}
                    />
                    {m.label}
                  </label>
                ))}
              </div>
              <p className="desc">建议勾选 2-3 种，太多会生成海量组合</p>
              <div className="dict-actions">
                <button type="button" className="btn-small btn-primary" onClick={generateWord}>
                  生成并加入列表
                </button>
              </div>
            </section>
          </div>

          <section className="dict-section letter-section">
            <h3>高级选项（字母组合 / 批量字典）</h3>
            <p className="desc">
              勾选需要的组合类型，数字/字母可在任意位置；最长只能 3 位。字母组合 1-3 位：理论{" "}
              <strong>18,278</strong> 个（纯字母），超过上限按字典序截取
            </p>
            <div className="num-row">
              <label>
                最短长度
                <input
                  type="number"
                  min={1}
                  max={3}
                  value={minLen}
                  onChange={(e) => setMinLen(Math.max(1, Math.min(3, Number(e.target.value) || 1)))}
                />
              </label>
              <label>
                最长长度
                <input
                  type="number"
                  min={1}
                  max={3}
                  value={maxLen}
                  onChange={(e) => setMaxLen(Math.max(1, Math.min(3, Number(e.target.value) || 1)))}
                />
              </label>
              <label>
                生成数量上限
                <input
                  type="number"
                  min={1}
                  max={100000}
                  value={cap}
                  onChange={(e) =>
                    setCap(Math.max(1, Math.min(100000, Number(e.target.value) || 1)))
                  }
                />
              </label>
            </div>
            {[1, 2, 3].map(
              (len) =>
                len >= minLen &&
                len <= maxLen && (
                  <div key={len} className="pattern-group">
                    <span className="pattern-title">{len} 位</span>
                    <div className="checkbox-grid">
                      {PATTERNS[len].map((p) => (
                        <label key={p.id}>
                          <input
                            type="checkbox"
                            checked={letterTypes.includes(p.id)}
                            onChange={() => toggleLetterType(p.id)}
                          />
                          {p.label}
                        </label>
                      ))}
                    </div>
                  </div>
                ),
            )}
            <p className="desc">
              预计生成 <strong>{letterEstimate}</strong> 个，去重后新增约{" "}
              <strong>{letterEstimate}</strong> 个
            </p>
            <div className="dict-actions">
              <button type="button" className="btn-small btn-primary" onClick={generateLetter}>
                生成并加入列表
              </button>
            </div>
          </section>

          <section className="dict-section">
            <h3>字典列表</h3>
            <div className="dict-toolbar">
              <span className="desc">
                共 <strong>{dict.length}</strong> 条
              </span>
              <div className="dict-actions">
                <button type="button" className="btn-small" onClick={importDictToQuery}>
                  加入查询列表
                </button>
                <button type="button" className="btn-small" onClick={exportDict}>
                  导出 .txt…
                </button>
                <button type="button" className="btn-small" onClick={copyDict}>
                  复制全部
                </button>
                <button
                  type="button"
                  className="btn-small btn-danger"
                  onClick={() => {
                    setDict([]);
                    setDictMsg("已清空");
                  }}
                >
                  清空
                </button>
              </div>
            </div>
            {dictMsg && <p className="dict-msg">{dictMsg}</p>}
            <pre className="dict-preview">
              {dict.length
                ? dict.slice(0, 500).join("\n") + (dict.length > 500 ? `\n…（共 ${dict.length} 条）` : "")
                : "（空）"}
            </pre>
          </section>
        </main>
      )}
    </div>
  );
}

