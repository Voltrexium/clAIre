import { useEffect, useState, type ReactNode } from "react";
import { getSettings, openStorageFolder, saveSettings, storageInfo } from "../shared/api";
import {
  ANTHROPIC_MODELS,
  OLLAMA_MODELS,
  OPENAI_MODELS,
  PROVIDER_PRESETS,
  PROVIDERS,
  withCurrent,
} from "../shared/models";
import type { Provider, Settings, StorageInfo } from "../shared/types";

const DEFAULTS: Settings = {
  provider: "openai",
  openaiApiKey: "",
  openaiModel: "gpt-5.4-mini",
  openaiBaseUrl: "https://api.openai.com/v1",
  anthropicApiKey: "",
  anthropicModel: "claude-sonnet-5",
  ollamaBaseUrl: "http://127.0.0.1:11434",
  ollamaModel: "llava",
  customBaseUrl: "",
  customApiKey: "",
  customModel: "",
  systemPrompt:
    "You are clAIre, a fast desktop context assistant. The user may attach a screenshot of their screen or active window. Use that visual context. Be concise unless asked for depth. If web search results are provided, cite them briefly.",
  hotkey: "CommandOrControl+Shift+Space",
  captureMode: "current",
  captureDisplayIds: [],
  webSearchEnabled: false,
  googleApiKey: "",
  googleCx: "",
  historyLimit: 6,
  downscaleMaxWidth: 1280,
};

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
    </label>
  );
}

function Text({
  label,
  value,
  onChange,
  type = "text",
  placeholder,
}: {
  label: string;
  value: string | number;
  onChange: (value: string) => void;
  type?: string;
  placeholder?: string;
}) {
  return (
    <Field label={label}>
      <input type={type} value={value} placeholder={placeholder} onChange={(e) => onChange(e.target.value)} />
    </Field>
  );
}

function ModelSelect({
  label,
  value,
  models,
  onChange,
}: {
  label: string;
  value: string;
  models: string[];
  onChange: (value: string) => void;
}) {
  return (
    <Field label={label}>
      <select value={value} onChange={(e) => onChange(e.target.value)}>
        {withCurrent(models, value).map((model) => (
          <option key={model} value={model}>
            {model}
          </option>
        ))}
      </select>
    </Field>
  );
}

export default function SettingsPage({ onClose }: { onClose?: () => void }) {
  const [settings, setSettings] = useState<Settings>(DEFAULTS);
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const patch = <K extends keyof Settings>(key: K, value: Settings[K]) =>
    setSettings((current) => ({ ...current, [key]: value }));

  useEffect(() => {
    void (async () => {
      try {
        const [loaded, storage] = await Promise.all([getSettings(), storageInfo()]);
        setSettings({ ...DEFAULTS, ...loaded });
        setInfo(storage);
      } catch (err) {
        setStatus(String(err));
      }
    })();
  }, []);

  function onProvider(next: Provider) {
    setSettings((current) => {
      const preset = PROVIDER_PRESETS[next];
      if (!preset) return { ...current, provider: next };
      return { ...current, provider: next, customBaseUrl: preset.base, customModel: preset.defaultModel };
    });
  }

  async function onSave() {
    setBusy(true);
    try {
      const saved = await saveSettings(settings);
      setSettings(saved);
      setInfo(await storageInfo());
      setStatus("Saved.");
    } catch (err) {
      setStatus(String(err));
    } finally {
      setBusy(false);
    }
  }

  const preset = PROVIDER_PRESETS[settings.provider];

  return (
    <div className="settings-shell">
      <div className="settings-toolbar">
        {onClose && (
          <button className="ghost" type="button" onClick={onClose}>
            Back
          </button>
        )}
        <button className="primary" type="button" disabled={busy} onClick={() => void onSave()}>
          Save
        </button>
      </div>
      {status && <div className="status">{status}</div>}

      <section>
        <h2>Assistant</h2>
        <Field label="System prompt">
          <textarea rows={4} value={settings.systemPrompt} onChange={(e) => patch("systemPrompt", e.target.value)} />
        </Field>
        <div className="grid">
          <Text label="Global hotkey" value={settings.hotkey} onChange={(value) => patch("hotkey", value)} />
        </div>
      </section>

      <section>
        <h2>LLM provider</h2>
        <div className="grid">
          <Field label="Provider">
            <select value={settings.provider} onChange={(e) => onProvider(e.target.value as Provider)}>
              {PROVIDERS.map((provider) => (
                <option key={provider.id} value={provider.id}>
                  {provider.label}
                </option>
              ))}
            </select>
          </Field>
          <Text
            label="Recent messages sent with each query"
            type="number"
            value={settings.historyLimit}
            onChange={(value) => patch("historyLimit", Number(value))}
          />
        </div>

        {settings.provider === "openai" && (
          <div className="grid">
            <Text label="OpenAI API key" type="password" value={settings.openaiApiKey} onChange={(v) => patch("openaiApiKey", v)} />
            <ModelSelect label="Model" value={settings.openaiModel} models={OPENAI_MODELS} onChange={(v) => patch("openaiModel", v)} />
            <Text label="Base URL" value={settings.openaiBaseUrl} onChange={(v) => patch("openaiBaseUrl", v)} />
          </div>
        )}

        {settings.provider === "anthropic" && (
          <div className="grid">
            <Text label="Anthropic API key" type="password" value={settings.anthropicApiKey} onChange={(v) => patch("anthropicApiKey", v)} />
            <ModelSelect
              label="Model"
              value={settings.anthropicModel}
              models={ANTHROPIC_MODELS}
              onChange={(v) => patch("anthropicModel", v)}
            />
          </div>
        )}

        {settings.provider === "ollama" && (
          <div className="grid">
            <Text label="Ollama endpoint" value={settings.ollamaBaseUrl} onChange={(v) => patch("ollamaBaseUrl", v)} />
            <ModelSelect label="Model" value={settings.ollamaModel} models={OLLAMA_MODELS} onChange={(v) => patch("ollamaModel", v)} />
          </div>
        )}

        {(settings.provider === "custom" || preset) && (
          <div className="grid">
            <Text label="API key" type="password" value={settings.customApiKey} onChange={(v) => patch("customApiKey", v)} />
            {settings.provider === "custom" ? (
              <Text label="Model" value={settings.customModel} placeholder="model-id" onChange={(v) => patch("customModel", v)} />
            ) : (
              <ModelSelect
                label="Model"
                value={settings.customModel}
                models={preset?.models ?? []}
                onChange={(v) => patch("customModel", v)}
              />
            )}
            <Text
              label="Base URL"
              value={settings.customBaseUrl}
              placeholder="https://host/v1"
              onChange={(v) => patch("customBaseUrl", v)}
            />
          </div>
        )}
      </section>

      <section>
        <h2>Web search</h2>
        <label className="toggle">
          <input
            type="checkbox"
            checked={settings.webSearchEnabled}
            onChange={(e) => patch("webSearchEnabled", e.target.checked)}
          />
          Enable Google Programmable Search before the LLM call
        </label>
        <div className="grid">
          <Text label="Google API key" type="password" value={settings.googleApiKey} onChange={(v) => patch("googleApiKey", v)} />
          <Text label="Search engine ID (cx)" value={settings.googleCx} onChange={(v) => patch("googleCx", v)} />
        </div>
      </section>

      <section>
        <h2>Storage</h2>
        <p className="hint">
          Conversation history and the latest capture live in the platform app-data folder. Runtime memory only
          keeps the current session and the most recent screenshot.
        </p>
        {info && (
          <dl className="paths">
            <dt>App data</dt>
            <dd>{info.appDataDir}</dd>
            <dt>Settings</dt>
            <dd>{info.settingsPath}</dd>
            <dt>Context</dt>
            <dd>{info.contextDir}</dd>
            <dt>Stored turns</dt>
            <dd>{info.historyCount}</dd>
          </dl>
        )}
        <div className="grid">
          <Text
            label="Max screenshot width (px)"
            type="number"
            value={settings.downscaleMaxWidth}
            onChange={(v) => patch("downscaleMaxWidth", Number(v))}
          />
        </div>
        <button className="ghost" type="button" onClick={() => void openStorageFolder()}>
          Open storage folder
        </button>
      </section>
    </div>
  );
}
