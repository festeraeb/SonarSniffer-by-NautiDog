/**
 * LLM output translator — mirrors cesarops-forge-v2/src/translator.rs + MXFP4 MoE quirks.
 * Used by n8n Code nodes via eval(readFileSync(...)).
 *
 * Input: OpenAI chat completion, raw string, or { raw_output, content, reasoning_content, tool_call }
 * Output: { status, content, sanitized_content, tool_call, failure, raw_merged, model_hints }
 */

function normalizeLlmOutput(input) {
  const body = input?.body ?? input ?? {};
  let raw = pickRawText(body);
  const modelHint = body.model || body.model_id || guessModelHint(body, raw);

  raw = mergeMxfp4Fields(body, raw);
  const stripped = stripArtifacts(raw);
  const leak = hasPromptLeak(stripped);

  let toolCall = body.tool_call?.name ? body.tool_call : null;
  if (!toolCall && body.name && body.arguments) {
    toolCall = { name: body.name, arguments: body.arguments };
  }
  if (!toolCall) {
    toolCall = extractToolCall(stripped) || extractToolCall(raw);
  }

  let failure = null;
  if (leak) failure = 'prompt_leak';
  else if (!stripped.trim() && !toolCall) {
    if (isThinkOnly(raw)) failure = 'think_only';
    else failure = 'empty_response';
  } else if (body.tool_call_parse_error) {
    failure = 'malformed_tool_call';
  }

  const sanitized = sanitizeForUser(stripped, { dropThinkingPreamble: true });

  return {
    status: toolCall?.name ? 'tool_call' : failure ? 'failure' : 'content',
    content: stripped,
    sanitized_content: sanitized,
    tool_call: toolCall,
    tool_name: toolCall?.name || null,
    arguments: toolCall?.arguments || {},
    failure,
    raw_merged: raw,
    model_hint: modelHint,
    mxfp4_moe: /mxfp4|qwen3\.6|35b.*moe/i.test(modelHint || raw),
  };
}

function pickRawText(body) {
  if (typeof body === 'string') return body;
  if (body.raw_output) return String(body.raw_output);
  if (body.text) return String(body.text);
  if (body.content && typeof body.content === 'string') return body.content;
  const choice = body.choices?.[0] || body.choice;
  if (choice?.text) return String(choice.text);
  if (choice?.message) return mergeMessageFields(choice.message, true);
  if (body.message) return mergeMessageFields(body.message, true);
  return '';
}

function mergeMxfp4Fields(body, raw) {
  if (raw && raw.trim()) return raw;
  const rc = body.reasoning_content || body.choices?.[0]?.message?.reasoning_content;
  const c = body.content || body.choices?.[0]?.message?.content;
  return mergeMessageFields({ content: c || '', reasoning_content: rc }, true);
}

function mergeMessageFields(msg, mergeReasoning) {
  const c = (msg?.content || '').trim();
  const r = (msg?.reasoning_content || '').trim();
  if (c) {
    if (mergeReasoning && r && !c.includes(r)) {
      if (c.length < 120 && (r.includes('<tool_call>') || r.includes('```'))) {
        return `${msg.content}\n${r}`;
      }
    }
    return msg.content || '';
  }
  if (mergeReasoning && r) return r;
  return msg?.content || '';
}

function guessModelHint(body, raw) {
  return body.model || body.choices?.[0]?.model || (raw.includes('<|channel>') ? 'gemma-channel' : '');
}

function stripArtifacts(raw) {
  let s = String(raw || '');

  s = s.replace(/<\|channel>[a-zA-Z_]*\s*/gis, '');
  s = s.replace(/<channel\|>/g, '');
  s = s.replace(/^(?:final|answer|thought|commentary)\s*$/gim, '');
  s = s.replace(/^_thought\s*\n?/gm, '');

  s = s.replace(/<\|?tool_call\|?>/g, '<tool_call>');
  s = s.replace(/<\/\|?tool_call\|?>/g, '</tool_call>');

  s = s.replace(/<think>[\s\S]*?<\/redacted_thinking>/gi, '');
  s = s.replace(/<reasoning>[\s\S]*?<\/reasoning>/gi, '');
  s = s.replace(/<thought>[\s\S]*?<\/thought>/gi, '');
  s = s.replace(/<\/?thought>/gi, '');

  s = s.replace(/<\|im_start\|>/g, '');
  s = s.replace(/<\|redacted_im_end\|>/g, '');
  s = s.replace(/<\|endoftext\|>/g, '');

  s = collapseRepeatedLines(s);
  return s.trim();
}

function collapseRepeatedLines(s) {
  const lines = s.split('\n');
  if (lines.length < 4) return s;
  const out = [];
  const seen = new Map();
  for (const line of lines) {
    const t = line.trim();
    if (!t) {
      if (out.length && !out[out.length - 1].trim()) continue;
      out.push(line);
      continue;
    }
    const n = (seen.get(t) || 0) + 1;
    seen.set(t, n);
    if (n > 2) continue;
    out.push(line);
  }
  return out.join('\n');
}

function isThinkOnly(raw) {
  return (
    /<think>/i.test(raw) ||
    /<reasoning>/i.test(raw) ||
    /<\|channel>thought/i.test(raw) ||
    /^here'?s a thinking process:/im.test(raw)
  );
}

function hasPromptLeak(text) {
  const markers = [
    '<|im_start|>system',
    'You are a helpful',
    '<|im_start|>user',
    '### System:',
    '[INST]',
    '<|im_start|>assistant',
  ];
  return markers.some((m) => text.includes(m));
}

function sanitizeForUser(stripped, opts = {}) {
  let s = stripped;
  if (opts.dropThinkingPreamble) {
    s = s.replace(/^here'?s a thinking process:\s*/i, '');
    s = s.replace(/^\d+\.\s+\*\*[^*]+\*\*[^\n]*\n/gm, '');
  }
  return s.trim();
}

function extractToolCall(text) {
  if (!text) return null;
  const tag = text.match(/<tool_call>\s*([\s\S]*?)\s*<\/tool_call>/i);
  if (tag) return parseToolJson(tag[1].trim());

  const code = text.match(/```(?:json)?\s*(\{[\s\S]*?\})\s*```/i);
  if (code && code[1].includes('"name"')) return parseToolJson(code[1].trim());

  const idx = text.indexOf('{"name"');
  if (idx >= 0) {
    const slice = text.slice(idx);
    const end = findMatchingBrace(slice);
    if (end >= 0) {
      const candidate = slice.slice(0, end + 1);
      if (candidate.includes('arguments')) return parseToolJson(candidate);
    }
  }

  const trimmed = text.trim();
  if (trimmed.startsWith('{') && trimmed.endsWith('}') && trimmed.includes('"name"')) {
    return parseToolJson(trimmed);
  }

  const callColon = text.match(/call\s*:\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*\{([^}]*)\}/i);
  if (callColon) {
    const name = callColon[1];
    const inner = callColon[2];
    const args = {};
    const kvRe = /([a-zA-Z_][a-zA-Z0-9_]*)\s*:\s*"([^"]*)"/gi;
    let m;
    while ((m = kvRe.exec(inner))) args[m[1]] = m[2];
    return { name, arguments: args };
  }
  return null;
}

function parseToolJson(jsonStr) {
  try {
    const v = JSON.parse(jsonStr);
    if (v.name) return { name: v.name, arguments: v.arguments || {} };
  } catch (_) {
    /* fall through */
  }
  return null;
}

function findMatchingBrace(s) {
  let depth = 0;
  for (let i = 0; i < s.length; i++) {
    if (s[i] === '{') depth++;
    else if (s[i] === '}') {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

if (typeof module !== 'undefined') {
  module.exports = { normalizeLlmOutput };
}
