export type Provider =
  | "openai"
  | "anthropic"
  | "gemini"
  | "groq"
  | "openrouter"
  | "mistral"
  | "deepseek"
  | "xai"
  | "together"
  | "fireworks"
  | "ollama"
  | "custom";
export type CaptureMode = "current" | "all";

export type SearchProvider = "tavily" | "brave" | "duckduckgo";

export interface KeyUsage {
  month: string;
  count: number;
  monthlyLimit: number;
}

export interface SearchUsage {
  tavily: Record<string, KeyUsage>;
  brave: Record<string, KeyUsage>;
  duckduckgo: Record<string, KeyUsage>;
}

export interface Settings {
  provider: Provider;
  openaiApiKey: string;
  openaiModel: string;
  openaiBaseUrl: string;
  anthropicApiKey: string;
  anthropicModel: string;
  ollamaBaseUrl: string;
  ollamaModel: string;
  customBaseUrl: string;
  customApiKey: string;
  customModel: string;
  systemPrompt: string;
  hotkey: string;
  captureMode: CaptureMode;
  captureDisplayIds: number[];
  webSearchEnabled: boolean;
  searchProvider: SearchProvider;
  tavilyApiKey: string;
  braveApiKey: string;
  tavilyMonthlyLimit: number;
  braveMonthlyLimit: number;
  duckduckgoMonthlyLimit: number;
  searchUsage: SearchUsage;
  historyLimit: number;
  downscaleMaxWidth: number;
}

export interface DisplayInfo {
  id: number;
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  primary: boolean;
  current: boolean;
}

export interface CapturePayload {
  dataUrl: string;
  width: number;
  height: number;
  capturedAt: string;
  mode: string;
}

export interface StorageInfo {
  appDataDir: string;
  settingsPath: string;
  contextDir: string;
  historyCount: number;
}

export interface SearchSource {
  index: number;
  title: string;
  url: string;
}

export interface AskResult {
  answer: string;
  usedSearch: boolean;
  usedVision: boolean;
  searchProvider?: string | null;
  searchSources?: SearchSource[];
}

export interface AskStatus {
  phase: "search" | "llm" | "idle" | string;
  api: string;
  detail: string;
}
