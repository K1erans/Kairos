// Runs one Claude request for Kairos Desktop through the Claude Agent SDK, so it is billed to
// the Claude subscription this Mac is logged in to (`claude` > /login) and needs no API key.
// stdin:  {"system": string, "user": string, "effort": "low"|"medium"|"high", "model"?: string}
// stdout: the model's text. A failure prints one line to stderr and exits 1.
import { query } from "@anthropic-ai/claude-agent-sdk";

const chunks = [];
for await (const chunk of process.stdin) chunks.push(chunk);
const request = JSON.parse(Buffer.concat(chunks).toString("utf8"));

// An API key in the environment would take precedence over the subscription login.
const { ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN, ...env } = process.env;

const abort = new AbortController();
process.on("SIGTERM", () => abort.abort());

try {
  let text = null;
  for await (const message of query({
    prompt: request.user,
    options: {
      systemPrompt: request.system,
      model: request.model ?? "claude-opus-5-5",
      effort: request.effort ?? "high",
      tools: [],
      maxTurns: 1,
      settingSources: [],
      persistSession: false,
      abortController: abort,
      env,
    },
  })) {
    if (message.type !== "result") continue;
    if (message.subtype !== "success") throw new Error(`Claude stopped early (${message.subtype}).`);
    text = message.result;
  }
  if (!text || !text.trim()) throw new Error("Claude returned nothing.");
  process.stdout.write(text);
} catch (error) {
  process.stderr.write(`${error?.message ?? error}\n`);
  process.exit(1);
}
