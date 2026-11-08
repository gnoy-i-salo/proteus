# Владельцы кода и применение стандарта

Этот путеводитель описывает фактические модули, а не обязательную схему будущих
каталогов. Межпакетные зависимости задаёт [LikeC4](../architecture/proteus.c4),
имена и границы — [стандарт](code-style.md), версии и восстановление —
[совместимость](code-compatibility.md), команды — [harness](harness.md).

## Пакеты

Все внешние consumers импортируют публичный `@proteus-int/<package>` API;
`index.ts` содержит explicit reexports. Внутри пакета импорт идёт к владельцу,
локальный parser не становится публичным только ради теста или переноса.

| Область                | Владельцы                                                                                                                                                                                                                                                              |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Контракты и версии     | [contracts](../packages/contracts/src/index.ts): предметные DTO отдельно; `*-codec.ts` и `*-validation.ts` проверяют свои схемы, а не произвольный JSON                                                                                                                |
| Провайдеры и настройки | [intelligence](../packages/intelligence/src/index.ts): provider, structured response и model-settings codec                                                                                                                                                            |
| Исследование           | [stalker](../packages/stalker/src/index.ts): public-research, source-collection, source-analysis; чистые parsers и source scope/budget рядом                                                                                                                           |
| Профиль и адаптация    | [fabricate-person](../packages/person-fabric/src/fabricate-person.ts), [adapt-person](../packages/person-fabric/src/adapt-person.ts), [parse-rules](../packages/person-fabric/src/parse-rules.ts), [neutral-profile](../packages/person-fabric/src/neutral-profile.ts) |
| Ответ и артефакты      | [person](../packages/person/src/index.ts): solve-task, parse-reply, persona-artifacts и port                                                                                                                                                                           |
| Рубрика и оценка       | [brain-analyzator](../packages/brain-analyzator/src/index.ts): rubric/ и answers/                                                                                                                                                                                      |
| Детерминированный BSC  | [brain-scoring](../packages/brain-scoring/src/index.ts): score-trial, summarize-scores, score-interval, validate-rubric; legacy-score изолирован                                                                                                                       |
| Оркестрация и экспорт  | [pipeline](../packages/pipeline/src/index.ts): experiment/, execution/, presets/, export/; protocol и immutable input snapshot принадлежат experiment                                                                                                                  |

Не создавать новый package для одноразового parser/helper. Плоская структура
STALKER и Person Fabric соответствует независимым обязанностям; вложенные
каталоги не являются самостоятельным критерием качества.

## UI и native

[ConnectionSession](../apps/desktop/src/features/connections/model/connection-session.ts)
владеет выбранным провайдером, polling, revision/disposal, состоянием вызова и QR.
[ConnectionMaterials](../apps/desktop/src/features/connections/model/connection-materials.ts)
владеет импортом и подтверждением удаления. UI держит локальное раскрытие формы;
модель получает getter текущего выбора и использует тот же activity gate.
Не добавлять отдельный `connection-polling.ts`, если это лишь дробит одну lifetime.

[ModelSettingsEditor](../apps/desktop/src/features/model-settings/model/model-settings-editor.ts)
получает getters, поэтому новые props не превращаются в устаревший snapshot.
Таймеры и подписки создаются внутри reactive owner SolidJS 1.9 и имеют cleanup;
React hooks и глобальный controller не заменяют эти условия.

[connections.rs](../apps/desktop/src-tauri/src/connections.rs) содержит thin Tauri
wrappers; [connection-session.rs](../apps/desktop/src-tauri/src/connection-session.rs)
и provider-модули выполняют сценарии. Собственные Rust DTO сериализуются в camelCase,
но OAuth `access_token`, TDLib `@type` и прочие внешние ключи принадлежат адаптерам.
[wire fixtures](../apps/desktop/src-tauri/src/connection-wire-tests.rs) и
[TS codec](../apps/desktop/src/shared/lib/connection-wire-codec.ts) проверяют границу.
Не читать реальные credential files для проверки naming.

[SourceStore](../apps/desktop/src-tauri/src/source-store.rs) владеет открытием базы
и legacy import; [CorpusDatabase](../apps/desktop/src-tauri/src/corpus-database.rs)
— SQLCipher, immutable revisions и транзакциями. Нативные ingestion-сценарии
проверяют текущие разрешения; storage API не становится авторизацией.
[FTS](../apps/desktop/src-tauri/src/corpus-search.rs),
[подготовка](local-tokenizer.md) и [CPU inference](local-embedding.md) имеют
разные обязанности. Это общие реализации для RAG и UX, без второго MemoryPort.

## Проверка применения навыка

Ручные упражнения используют текущие файлы и синтетические fixtures. Это оценка
решений по конкретным примерам, а не доказательство автоматического соблюдения
навыка любым агентом. Повторная проверка итогового дерева обязательна в gx9.26.

| Запрос                                | Выбор владельца и наблюдаемая проверка                                                                                                                                        | Отклонённый вариант                                                                 |
| ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| Извлечь lifecycle подключений         | Сохранить connection-session и connection-materials; существующие tests проверяют stale poll после disconnect/import, cleanup, deadline GitHub и поздний ответ после disposal | Перенос той же большой функции в global store; потеря revision guard                |
| Отделить evidence parser Fabric       | Оставить parse-rules внутренним; person-fabric tests сохраняют task-free baseline, evidence IDs, legacy exact text и отказ при mixed aliases                                  | Новый npm package или экспорт parser только ради импортов                           |
| Переименовать своё native timing-поле | Rust `poll_after_ms`, wire `pollAfterMs`; общая TS/Rust fixture, legacy decoder отдельно                                                                                      | Переименование внешнего `access_token` или перезапись старого секрета ради проверки |
| Перевести старый CSS modifier         | `modelSettings__row` плюс `modelSettings__row_active`; Stylelint fixtures проверяют grammar и tokens                                                                          | Modifier без base, CSS `--active`, замена property names/исходных строк             |
| Прочитать старый preset с цитатой     | persona-preset-codec читает v1/v2 с exact Unicode/whitespace, отклоняет конфликтующие aliases и неизвестную версию                                                            | Recursive rename произвольного JSON или обещание отсутствующего result importer     |
| Упростить короткий чистый helper      | Сохранить рядом с владельцем, если нет отдельной ответственности                                                                                                              | Новая hierarchy или layer только ради уменьшения числа строк                        |

Фактические результаты прогонов и ещё не выполненные release checks записываются
в Beads, а не выводятся из наличия этих файлов. External/live provider calls,
доступ к архивам и установка приложения для этих упражнений не требуются.
