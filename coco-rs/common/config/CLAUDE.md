# coco-config

Layered configuration: settings, model selection, providers, effort, fast mode.

## TS Source
- `src/utils/settings/` (types.ts, settings.ts -- Zod schema -> serde)
- `src/utils/model/` (model.ts, configs.ts, aliases.ts, providers.ts, modelCapabilities.ts)
- `src/constants/`
- `src/migrations/`
- `src/utils/effort.ts`, `src/utils/fastMode.ts`, `src/utils/thinking.ts`
- `src/services/remoteManagedSettings/`, `src/services/settingsSync/`
- `src/utils/envUtils.ts`, `src/utils/gitSettings.ts`, `src/utils/lockfile.ts`

## Key Types
Settings, GlobalConfig, ModelInfo, ProviderInfo, ModelRoles, ModelAlias, FastModeState, SettingsWatcher, ConfigLoader
