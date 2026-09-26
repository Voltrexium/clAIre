# Privacy Policy for clAIre

Last updated: September 26, 2026

## 1. Overview
clAIre is an open-source application designed with privacy as a core principle. The app runs locally on your machine and does not route telemetry, screen captures, or prompts through any project-owned intermediate servers.

## 2. Data Handled by the Application
* **Screen Captures & Prompts:** When you send a question and a capture exists, the screenshot and your prompt go directly from your device to the chat provider you configured (OpenAI, Anthropic, Gemini, Groq, OpenRouter, Mistral, DeepSeek, xAI, Together, Fireworks, a custom OpenAI-compatible server, or a local Ollama instance). Each chat request also includes a short local machine description: operating system, CPU, memory, and hostname.
* **Password fields:** By default, password controls reported by the operating system are covered in the image before it is saved or sent. macOS needs Accessibility permission for that lookup. Text typed in a terminal, or drawn by an app that does not expose its fields, is not covered. You can turn this off in Settings.
* **Web search:** If web search is enabled and the overlay Web switch is on, the question text is sent to Tavily, Brave, or DuckDuckGo before the chat request. The screenshot is not sent to the search provider. A search error is shown locally and the chat request still goes out without search results.
* **API Credentials & Settings:** Provider and search keys are stored in the operating system credential store (macOS Keychain, Windows Credential Manager, or the Linux Secret Service). Other settings, such as the hotkey and model name, stay in the local app-data directory. Search usage is counted under a SHA-256 id of the key, not the key itself. clAIre does not send keys to a project server.
* **Local development overrides:** If a `.env` file or the process environment contains `API_KEY` or `GEMINI_API_KEY`, clAIre switches the chat provider to Gemini on launch. Search variables (`TAVILY_API_KEY`, `BRAVE_API_KEY`, `API_KEY_SEARCH`, `SEARCH_PROVIDER`) can turn web search on and fill those keys. Those files stay on your machine.

## 3. Third-Party Services
Requests sent to third-party endpoints are governed by their respective privacy policies. Which services receive data depends on the providers you enable:
* [OpenAI Privacy Policy](https://openai.com/privacy)
* [Anthropic Privacy Policy](https://www.anthropic.com/privacy)
* [Google Privacy Policy](https://policies.google.com/privacy) (Gemini)
* [Groq Privacy Policy](https://groq.com/privacy-policy)
* [OpenRouter Privacy Policy](https://openrouter.ai/privacy)
* [Mistral Privacy Policy](https://mistral.ai/legal/privacy)
* [DeepSeek Privacy Policy](https://cdn.deepseek.com/policies/en-US/deepseek-privacy-policy.html)
* [xAI Privacy Policy](https://x.ai/legal/privacy-policy)
* [Together Privacy Policy](https://www.together.ai/privacy)
* [Fireworks Privacy Policy](https://fireworks.ai/privacy-policy)
* [Tavily Privacy Policy](https://www.tavily.com/privacy)
* [Brave Privacy Policy](https://brave.com/privacy/)
* [DuckDuckGo Privacy Policy](https://duckduckgo.com/privacy)

Ollama requests stay on the endpoint you configure, which is `http://127.0.0.1:11434` by default.

## 4. Local Storage & Retention
Session history and the latest screenshot stay in the app-data `context/` directory (`session.json`, `latest.png`, and `latest.json`) until you choose **Clear context** from the overlay or the tray. Hiding the overlay, pressing Esc, or pressing the hotkey again clears the chat and leaves the screenshot files on disk. **New chat** clears the thread and keeps the current screenshot. Uninstalling or deleting the app-data directory removes what remains.

## 5. Contact
For privacy questions or concerns, please open a GitHub issue or contact voltrexium@gmail.com.
