import { useEffect, useState, type ReactNode } from "react";
import {
  getSettings,
  openStorageFolder,
  saveSettings,
  storageInfo,
} from "../shared/api";
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

function Field({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
    </label>
  );
}

export default function SettingsPage({ onClose }: { onClose?: () => void }) {
  const [settings, setSettings] = useState<Settings>(DEFAULTS);
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

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

  function patch<K extends keyof Settings>(key: K, value: Settings[K]) {
    setSettings((current) => ({ ...current, [key]: value }));
  }

  function onProvider(next: Provider) {
    setSettings((current) => {
      const preset = PROVIDER_PRESETS[next];
      if (!preset) {
        return { ...current, provider: next };
      }
      return {
        ...current,
        provider: next,
        customBaseUrl: preset.base,
        customModel: preset.defaultModel,
      };
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
          <textarea
            rows={4}
            value={settings.systemPrompt}
            onChange={(e) => patch("systemPrompt", e.target.value)}
          />
        </Field>
      </section>

      <section>
        <h2>LLM provider</h2>
        <div className="grid">
          <Field label="Provider">
            <select
              value={settings.provider}
              onChange={(e) => onProvider(e.target.value as Provider)}
            >
              {PROVIDERS.map((provider) => (
                <option key={provider.id} value={provider.id}>
                  {provider.label}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Recent messages sent with each query">
            <input
              type="number"
              min={0}
              max={32}
              value={settings.historyLimit}
              onChange={(e) => patch("historyLimit", Number(e.target.value))}
            />
          </Field>
        </div>

        {settings.provider === "openai" && (
          <div className="grid">
            <Field label="OpenAI API key">
              <input
                type="password"
                value={settings.openaiApiKey}
                onChange={(e) => patch("openaiApiKey", e.target.value)}
              />
            </Field>
            <Field label="Model">
              <select value={settings.openaiModel} onChange={(e) => patch("openaiModel", e.target.value)}>
                {withCurrent(OPENAI_MODELS, settings.openaiModel).map((model) => (
                  <option key={model} value={model}>
                    {model}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="Base URL">
              <input value={settings.openaiBaseUrl} onChange={(e) => patch("openaiBaseUrl", e.target.value)} />
            </Field>
          </div>
        )}

        {settings.provider === "anthropic" && (
          <div className="grid">
            <Field label="Anthropic API key">
              <input
                type="password"
                value={settings.anthropicApiKey}
                onChange={(e) => patch("anthropicApiKey", e.target.value)}
              />
            </Field>
            <Field label="Model">
              <select value={settings.anthropicModel} onChange={(e) => patch("anthropicModel", e.target.value)}>
                {withCurrent(ANTHROPIC_MODELS, settings.anthropicModel).map((model) => (
                  <option key={model} value={model}>
                    {model}
                  </option>
                ))}
              </select>
            </Field>
          </div>
        )}

        {settings.provider === "ollama" && (
          <div className="grid">
            <Field label="Ollama endpoint">
              <input value={settings.ollamaBaseUrl} onChange={(e) => patch("ollamaBaseUrl", e.target.value)} />
            </Field>
            <Field label="Model">
              <select value={settings.ollamaModel} onChange={(e) => patch("ollamaModel", e.target.value)}>
                {withCurrent(OLLAMA_MODELS, settings.ollamaModel).map((model) => (
                  <option key={model} value={model}>
                    {model}
                  </option>
                ))}
              </select>
            </Field>
          </div>
        )}

        {settings.provider === "custom" || PROVIDER_PRESETS[settings.provider] ? (
          <div className="grid">
            <Field label="API key">
              <input
                type="password"
                value={settings.customApiKey}
                onChange={(e) => patch("customApiKey", e.target.value)}
              />
            </Field>
            <Field label="Model">
              {settings.provider === "custom" ? (
                <input
                  value={settings.customModel}
                  placeholder="model-id"
                  onChange={(e) => patch("customModel", e.target.value)}
                />
              ) : (
                <select value={settings.customModel} onChange={(e) => patch("customModel", e.target.value)}>
                  {withCurrent(
                    PROVIDER_PRESETS[settings.provider]?.models ?? [],
                    settings.customModel,
                  ).map((model) => (
                    <option key={model} value={model}>
                      {model}
                    </option>
                  ))}
                </select>
              )}
            </Field>
            <Field label="Base URL">
              <input
                value={settings.customBaseUrl}
                placeholder="https://host/v1"
                onChange={(e) => patch("customBaseUrl", e.target.value)}
              />
            </Field>
          </div>
        ) : null}
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
          <Field label="Google API key">
            <input
              type="password"
              value={settings.googleApiKey}
              onChange={(e) => patch("googleApiKey", e.target.value)}
            />
          </Field>
          <Field label="Search engine ID (cx)">
            <input value={settings.googleCx} onChange={(e) => patch("googleCx", e.target.value)} />
          </Field>
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
          <Field label="Max screenshot width (px)">
            <input
              type="number"
              min={640}
              max={3840}
              value={settings.downscaleMaxWidth}
              onChange={(e) => patch("downscaleMaxWidth", Number(e.target.value))}
            />
          </Field>
        </div>
        <button className="ghost" type="button" onClick={() => void openStorageFolder()}>
          Open storage folder
        </button>
      </section>
    </div>
  );
}
