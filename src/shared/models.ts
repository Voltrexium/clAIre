/** Hardcoded provider model lists (updated Sep 2026). Base URLs live in providerBases.json so the Rust client uses the same hosts. */
import type { Provider } from "./types";
import bases from "./providerBases.json";

export const OPENAI_MODELS = [
  "gpt-5.4",
  "gpt-5.4-mini",
  "gpt-5.4-nano",
  "gpt-5",
  "gpt-5-mini",
  "gpt-5-nano",
  "gpt-4.1",
  "gpt-4.1-mini",
  "gpt-4.1-nano",
  "gpt-4o",
  "gpt-4o-mini",
  "o4-mini",
  "o3",
  "o3-mini",
];

export const ANTHROPIC_MODELS = [
  "claude-opus-5",
  "claude-sonnet-5",
  "claude-haiku-5",
  "claude-opus-4-6",
  "claude-sonnet-4-6",
  "claude-opus-4-5",
  "claude-sonnet-4-5",
  "claude-haiku-4-5",
  "claude-opus-4-1",
  "claude-sonnet-4",
  "claude-3-5-haiku-latest",
];

export const GEMINI_MODELS = [
  "gemini-2.5-pro",
  "gemini-2.5-flash",
  "gemini-2.5-flash-lite",
  "gemini-2.5-flash-image",
  "gemini-2.0-flash",
  "gemini-2.0-flash-lite",
];

export const GROQ_MODELS = [
  "llama-3.3-70b-versatile",
  "llama-3.1-8b-instant",
  "meta-llama/llama-4-scout-17b-16e-instruct",
  "meta-llama/llama-4-maverick-17b-128e-instruct",
  "openai/gpt-oss-120b",
  "qwen/qwen3-32b",
];

export const OPENROUTER_MODELS = [
  "openai/gpt-5.4",
  "openai/gpt-4o",
  "anthropic/claude-sonnet-4.5",
  "google/gemini-2.5-flash",
  "meta-llama/llama-4-maverick",
  "qwen/qwen3-32b",
];

export const MISTRAL_MODELS = [
  "mistral-large-latest",
  "mistral-medium-latest",
  "mistral-small-latest",
  "pixtral-large-latest",
  "pixtral-12b-latest",
  "codestral-latest",
];

export const DEEPSEEK_MODELS = [
  "deepseek-chat",
  "deepseek-reasoner",
];

export const XAI_MODELS = [
  "grok-4",
  "grok-3",
  "grok-3-mini",
  "grok-2-vision-1212",
];

export const TOGETHER_MODELS = [
  "meta-llama/Llama-4-Maverick-17B-128E-Instruct-FP8",
  "meta-llama/Llama-3.3-70B-Instruct-Turbo",
  "Qwen/Qwen3-235B-A22B-Instruct",
  "deepseek-ai/DeepSeek-V3",
];

export const FIREWORKS_MODELS = [
  "accounts/fireworks/models/llama-v3p3-70b-instruct",
  "accounts/fireworks/models/llama4-maverick-instruct-basic",
  "accounts/fireworks/models/qwen3-235b-a22b",
  "accounts/fireworks/models/deepseek-v3",
];

export const OLLAMA_MODELS = [
  "llava",
  "llama3.2-vision",
  "qwen2.5vl",
  "minicpm-v",
  "gemma3",
  "mistral-small3.1",
  "llama3.3",
];

export const PROVIDERS: Array<{ id: Provider; label: string }> = [
  { id: "openai", label: "OpenAI" },
  { id: "anthropic", label: "Anthropic" },
  { id: "gemini", label: "Google Gemini" },
  { id: "groq", label: "Groq" },
  { id: "openrouter", label: "OpenRouter" },
  { id: "mistral", label: "Mistral" },
  { id: "deepseek", label: "DeepSeek" },
  { id: "xai", label: "xAI Grok" },
  { id: "together", label: "Together AI" },
  { id: "fireworks", label: "Fireworks" },
  { id: "ollama", label: "Ollama / local" },
  { id: "custom", label: "Custom OpenAI-compatible" },
];

export const PROVIDER_PRESETS: Partial<
  Record<Provider, { base: string; models: string[]; defaultModel: string }>
> = {
  gemini: {
    base: bases.gemini,
    models: GEMINI_MODELS,
    defaultModel: "gemini-2.5-flash",
  },
  groq: {
    base: bases.groq,
    models: GROQ_MODELS,
    defaultModel: "llama-3.3-70b-versatile",
  },
  openrouter: {
    base: bases.openrouter,
    models: OPENROUTER_MODELS,
    defaultModel: "openai/gpt-4o",
  },
  mistral: {
    base: bases.mistral,
    models: MISTRAL_MODELS,
    defaultModel: "mistral-small-latest",
  },
  deepseek: {
    base: bases.deepseek,
    models: DEEPSEEK_MODELS,
    defaultModel: "deepseek-chat",
  },
  xai: {
    base: bases.xai,
    models: XAI_MODELS,
    defaultModel: "grok-3-mini",
  },
  together: {
    base: bases.together,
    models: TOGETHER_MODELS,
    defaultModel: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
  },
  fireworks: {
    base: bases.fireworks,
    models: FIREWORKS_MODELS,
    defaultModel: "accounts/fireworks/models/llama-v3p3-70b-instruct",
  },
};

export function withCurrent(models: string[], current: string) {
  if (current && !models.includes(current)) {
    return [current, ...models];
  }
  return models;
}
