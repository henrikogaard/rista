// ── AI Provider definitions ─────────────────────────────────────
export const PROVIDERS = {
  anthropic: {
    label: 'Anthropic',
    defaultModel: 'claude-sonnet-4-5-20250929',
    models: ['claude-opus-4-5-20251029', 'claude-sonnet-4-5-20250929', 'claude-haiku-4-5-20251001'],
    defaultBaseUrl: 'https://api.anthropic.com',
  },
  openai: {
    label: 'OpenAI',
    defaultModel: 'gpt-4o',
    models: ['gpt-4o', 'gpt-4o-mini', 'gpt-4-turbo'],
    defaultBaseUrl: 'https://api.openai.com',
  },
  ollama: {
    label: 'Ollama',
    defaultModel: 'llama3',
    models: ['llama3', 'mistral', 'codellama'],
    defaultBaseUrl: 'http://localhost:11434',
    noApiKey: true,
  },
}
