# Совместимость схем при переходе на camelCase

Срез исходного дерева: `2a18fe4453cc307cbbe65502f2ec9a5435e5409b` плюс сохранённые исходные изменения source identity / подключений. Это спецификация границ; готовность реализации и проверки находятся в Beads. Нельзя считать таблицу доказательством выполненной миграции.

## Реализованный переход gx9.6 / gx9.14

Внутренний TypeScript и live IPC используют camelCase; Rust identifiers остаются snake_case. Сохранены command/event IDs, ComponentId/status values и внешние поля провайдеров. IPC не является форматом исторического хранения: frontend и Rust поставляются одной сборкой, обе стороны проверяются общей несекретной fixture. Legacy IPC converters остаются отдельными проверенными границами, без разрешения смешанных схем в live DTO.

- Настройки моделей: envelope `proteus-model-settings`, version 2. Исходный localStorage ключ сохраняется, ошибка восстановления отображается явно. `.proteus` пишет version 2 с `publicUrls`, читает v1 и v2 без изменения текста.
- Результат: `schemaVersion: 4`, `scoringVersion: 2`, protocol version 3, `promptVersion: bsc-v2.4-camel-case`. Style profile, reply и envelope `proteus-style-exemplars` имеют version 2. Нативное чтение истории и отдельные декодеры unversioned v1, kebab v2/v3 и camelCase v4 подключены к просмотру; итоговая приёмка x1y.16 остаётся отдельным условием. Старые файлы не переписываются.
- Native secrets: `proteus-connection-secret`, version 2, kind `config | token | githubPending | vkPending`, `payload`; предел 1 МиБ. Четыре legacy-схемы читаются без записи или копирования секрета; новая запись проверяется перед заменой того же элемента OS store.
- Корпус: `proteus-source-record`, version 2, kind `grant | import | deleted`, `payload`, отдельный путь `v2/<id>.enc`. Legacy Record v1 и unversioned imports читаются явно; старые encrypted originals сохраняются. PRT1, AAD и ключ `corpus-key-v1` неизменны. Повреждённое или неизвестное новое поколение не вызывает возврат к старому. Tombstone предотвращает восстановление отозванных данных при прерванном удалении; повтор удаления завершает обе версии.

Запись ciphertext проверяется до rename. На Unix синхронизируются файлы и метаданные каталога; после успешного rename и ошибки последующего sync новая корректная версия уже может быть видна, поэтому повтор безопасен, но ошибка не означает откат публикации. Эти fixture-проверки не моделируют потерю питания; универсальная power-loss гарантия для Windows не заявляется. Сборка и тесты каждой платформы остаются отдельными обязательными CI gates. На момент этого среза SQLCipher и immutable revisions относились к отдельной задаче tip.3; текущее состояние описано ниже.

Таблицы ниже фиксируют исходные схемы и владельцев на момент inventory; текущие файлы после декомпозиции перечислены в публичных `index.ts` и архитектуре.

## Исторический inventory форматов и владельцев

Граница [истории pipeline](../packages/pipeline/src/history/parse-history-envelope.ts)
ограничивает исходный UTF-8 JSON восемью МиБ и различает точные внешние envelopes
unversioned v1, kebab v2/v3 и camelCase v4. Определение версии не означает проверку
тела: каждую версию обязан принять её отдельный декодер перед отображением.
[Legacy v1](../packages/pipeline/src/history/legacy-v1.ts) возвращает отдельный
`LegacyV1Result` с camelCase-полями. Он сохраняет старые `bsc`, `reasoningMinutes`
и ratio как записанные значения; не добавляет protocol/control/trials/status и
не пересчитывает их по текущему BSC. Текст и непрозрачные ключи artifacts сохраняются,
в том числе `__proto__` как собственное поле данных. Карта не исполняется и не
интерпретируется как пути. Смешанные поля, неверные типы/даты и неизвестные версии
отклоняются без изменения исходного файла. Синтетические примеры и фактические
методы их получения перечислены в [provenance](../packages/pipeline/src/testing/legacy-results/provenance.md).

[V4](../packages/pipeline/src/history/result-v4.ts) читает raw и export формы,
сохраняет отсутствие необязательных execution/exportMetadata и точные строки.
Наблюдения lifecycle проверяет общий decoder contracts, включая реальные
календарные даты. Отдельная проверка связей отклоняет дубли/чужие ID участников,
прогонов и критериев, нарушение записанного порядка и противоречащие копии
ответов. Это проверка связей, а не повторная оценка: записанные BSC/интервалы,
ограничения экспорта и непрозрачные artifacts не пересоздаются. Тесты чтения
выполняют настоящий конвейер с тестовыми провайдерами для complete/incomplete/
cancelled, затем проверяют raw и фактический export. Они не вызывают внешние CLI.
Чтение не подтверждает актуальные разрешения источников.

История подключает v1–v4 через `readStoredResult`. Декодер `legacy-series` возвращает собственный camelCase тип v2/v3 с исходными protocol version 2, style/reply version 1; не превращает его в текущий ExperimentResult. Проверяются структуры, связи, полные копии trials/attempts при любом статусе и даты; записанные оценки, разницы и сводки не пересчитываются. Отсутствующие optional поля остаются отсутствующими. Для просмотра отдельная проекция применяет общую `source-export-policy`, сохраняя исходный decoded snapshot и JSON. Старая версия протокола показана явно, artifact-текст не регенерируется современным renderer.
`prepareStoredResult` отделяет сохранённый результат от проекции просмотра:
для v4 применяется существующий `exportExperiment`, включая исключение закрытых
примеров/цитат и preparation traces. Эта проекция может пересоздать производные
artifacts согласно текущей export policy; исходный JSON, его decoder result и
записанные оценки остаются неизменными. Известные ограничения поиска совпадающих
закрытых фрагментов действуют так же, как при экспорте; просмотр не объявляется
точным воспроизведением удалённого содержимого или проверкой актуальных grants.
UI отдельно показывает исходную версию/время/метаданные экспорта, не выдаёт
разрешений и не загружает источники. Сырой архив не предлагается для экспорта.
Явное действие «Новый запуск из этих условий» переносит разрешённую проекцию
ввода в форму, очищает прежние grants и требует обычного подтверждения нового
протокола при последующем старте. Открытие другого результата не меняет текущую
форму, ошибочный файл сохраняет ранее открытый результат.

| Формат                        | Чтение/запись исходного inventory                                                                                          | Новая граница                                                                                                                                                                                                                                                                                          | Владелец                                               |
| ----------------------------- | -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------ |
| Настройки моделей             | unversioned JSON, localStorage `proteus-model-settings`, `global-model`; parseModelSettings возвращает defaults при ошибке | Строгий подготовительный decoder legacy; новый envelope `format: proteus-model-settings`, `version: 2`, `globalModel`. Writer включается только вместе с consumers. Ошибка строгой миграции не должна стирать legacy запись.                                                                           | intelligence/model-settings-codec; desktop input model |
| `.proteus`                    | `proteus-personas`, version 1; pipeline/index + Rust presets.rs; 128 KiB; два PersonInput с public-urls                    | version 2 с publicUrls; legacy v1 читается схемно. Ввод не включает grants; новый UX template/manifest discriminator отдельно, не подмена v1.                                                                                                                                                          | pipeline presets + native presets                      |
| Сохранение/экспорт результата | Фактический writer schema-version **3**, scoring-version 2; Rust сохраняет JSON, текущего result importer нет              | Новый writer schemaVersion **4**. Reader history появляется только в x1y.16; старые v2/v3 read-only после отдельных fixtures. Не обещать существующего импорта.                                                                                                                                        | pipeline experiment/export + native save_experiment    |
| CLI IPC/stream                | Собственные kebab DTO cli.rs, shared/lib/cli.ts; upstream Codex/Kimi event JSON отдельно                                   | TS boundary adapter до переключения Rust, затем согласованный camelCase; строгие собственные IPC schemas и общие TS/Rust fixtures. IPC живёт внутри одной встроенной сборки frontend/Rust; версии envelope обязательны для persist-форматов. Внешние item_id/thread_id/token usage не переименовывать. | native cli, desktop shared/lib/cli                     |
| Source IPC                    | sources.rs, shared/lib/connections.ts; grant/action/result DTO                                                             | Явные converters известных полей, без рекурсивной обработки Value; Rust serde camelCase после TS switch                                                                                                                                                                                                | contracts/sources + native sources                     |
| Connection IPC                | connections.rs, social-oauth.rs; account/config/status DTO                                                                 | Отделить native-owned wire от provider DTO; legacy Keychain reader отдельно от camel writer                                                                                                                                                                                                            | native connections/oauth                               |
| Source storage                | PRT1 encrypted files, AAD PROTEUS-CORPUS-V1, corpus-key-v1; Record v1 + grants и unversioned ImportedRecord                | Сохранить encrypted originals; versioned SQLCipher migration tip.3, counts/hashes/links сверяются до atomic publish; naming не меняет ciphertext header/ключи                                                                                                                                          | native sources/social-storage                          |
| Model JSON, prompts, skills   | contracts + доменные parsers; prompt version bsc-v2.3-source-identity, исторический bsc-v2.2-source-style                  | Explicit legacy response decoder у владельца; новый canonical request/parser и prompt schema version переключаются одновременно; исходные цитаты и task неизменны                                                                                                                                      | intelligence/STALKER/Fabric/Person/Analyzator          |
| Persona artifacts             | AGENTS.md/SKILL.md/source-evidence.json/style-exemplars.json в generated map                                               | Меняются только schema-owned keys внутри известного JSON; artifact filenames, свободный Markdown, source text и historical prompt metadata не переписываются                                                                                                                                           | person/persona-artifacts                               |
| RAG/UX additions              | tip DTO/manifest/storage и x1y telemetry ещё не реализованы                                                                | С первого writer camelCase, явные format/version и source refs; не создавать вторые ports/storage                                                                                                                                                                                                      | contracts + владельцы tip/x1y                          |

Контракт `RetrievalReceipt` теперь имеет `schemaVersion: 2` и обязательную
`operation: recall | readEpisode`. Для `recall` нужен хотя бы один реальный ранг;
прямое чтение эпизода имеет оба ранга `null` и `fusionScore: 0`. `mode` сохраняет
лексическую/гибридную конфигурацию привязанной сессии и не подменяет тип операции.
Владелец декодирования — `contracts/memory-validation.ts`. Точная старая форма v1
принимается только как ranked recall и переводится в v2 в памяти; v1 с новым полем,
пустыми рангами, неизвестной версией или чужой областью отклоняется. Повторное
декодирование v2 идемпотентно, входной объект и ссылки на оригиналы не переписываются.
Native MemoryPort writer и persistent retrieval receipts ещё подключаются отдельно;
наличие декодера не означает реализованную миграцию существующего хранилища памяти.

## Нативные схемы, не попавшие в TypeScript AST

Ниже перечислены определения в исходниках, а не содержимое пользовательского Keychain или файлов корпуса. TS-таблица сама по себе не покрывает Rust-only persistence. В частности, существующая запись корпуса называется `Record`, а не `StoredSnapshot`; она имеет собственную версию 1 и содержит вложенные grant/document/coverage DTO. Смена их serde-ключей меняет и сохранённый формат, даже если внешний `Record.version` не изменился.

| Граница и Rust owner                                               | Точный legacy JSON / ключи                                                                                                                | Canonical mapping и стратегия                                                                                                                                                                                                                                  |
| ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `connections.rs::ConnectionConfig`, Keychain `<provider>-config`   | `provider`, `client-id`, `api-id`, `api-hash`, `token`; все поля Option, собственный serde kebab-case                                     | `clientId`, `apiId`, `apiHash`; остальные имена неизменны. Отдельный legacy storage DTO, новый versioned secret envelope; IPC config не становится reader старого Keychain автоматически. `apiHash`/`token` остаются только на нативной секретной границе.     |
| `connections.rs::Token`, `<provider>-token`                        | `access`, `refresh`, `device-id`, `expires-at`, `refreshed-at`, `account-id`, `label`                                                     | `deviceId`, `expiresAt`, `refreshedAt`, `accountId`; access/refresh/label неизменны. Здесь expires-at/refreshed-at — i64 timestamp, не строка SourceGrant.expires-at. Новый envelope после explicit legacy decode; не выводить значения в логи/fixtures/Beads. |
| `connections.rs::DevicePending`, `github-pending`                  | `client-id`, `device-code`, `user-code`, `expires-at`, `next-poll`, `interval`                                                            | `clientId`, `deviceCode`, `userCode`, `expiresAt`, `nextPoll`, `interval`. Сохранить срок действия, интервал и ограничения polling. Ошибка миграции не должна запускать повторный login или сбрасывать pending молча.                                          |
| `social-oauth.rs::VkPending`, `vk-pending`                         | `state`, `verifier`, `client-id`, `expires-at`                                                                                            | `state`, `verifier`, `clientId`, `expiresAt`. Сохранить одноразовость, PKCE и expiry; native-only versioned envelope и synthetic fixtures.                                                                                                                     |
| `sources.rs::Record`, `<grant-id>.enc`                             | `version: 1`, `grant`, `documents`, `coverage`, `cursor`, `checkpoint`, `steps`, **`started_at`**, `cancelled`, `limitations`             | `startedAt` — отдельный snake_case legacy key, не `started-at`. Остальные внешние ключи без переименования; вложенные DTO мигрируются по своим схемам. Новое поколение storage получает отдельную версию; v1 decoder остаётся явным.                           |
| `sources.rs::Checkpoint` внутри Record                             | `phase`, `offset`, `cursor`, `index`, `queue`                                                                                             | Поля уже совместимы; содержимое `queue: Vec<serde_json::Value>` — внешние provider payloads. Не обходить очередь recursive rename, не переписывать cursor/phase значения.                                                                                      |
| `sources.rs::ImportedRecord`, `<import-id>.enc`                    | unversioned `{provider, documents, warnings}`                                                                                             | Оболочка и вложенные documents имеют отдельный legacy decoder. Новый versioned storage discriminator отличает импорт от grant snapshot; наличие похожего набора полей не должно превращать импорт в grant.                                                     |
| `sources.rs::SourceGrant`                                          | flatten SourceGrantRequest плюс `id`, `account-id`, `snapshot-id`, `expires-at`                                                           | `accountId`, `snapshotId`, `expiresAt`; request сохраняет плоскую wire форму. `expiresAt` здесь строка. Optional author/from/to и budget/operations проверяются независимо.                                                                                    |
| `sources.rs::SourceDocument/SourceCoverage`                        | известные kebab keys из TS-таблицы; Rust Option-поля сериализуются как null, включая author/source URL/time/reply и total-documents       | camelCase по explicit DTO, null не заменять пустой строкой или отсутствием. Не терять `authorship`, `partition`, `truncated`, `is-forwarded`, точный text и content-hash.                                                                                      |
| `sources.rs::SourceAction`                                         | tagged union по `action`; `describe`, `coverage`, `search`, `collect`, `read`; Read явно задаёт `document-ids`                            | Discriminator и значения операций неизменны. `documentIds` требует отдельного изменения serde rename поля варианта: один rename_all enum не переименует его автоматически.                                                                                     |
| `cli.rs::Request/Event/Trace`                                      | `system-prompt`, `source-context`; `elapsed-ms`; trace keys из TS-таблицы. Request defaults: operation=`generation`, source-context=false | Own camelCase wire с согласованным frontend. Сохранить defaults, absent reasoning в Event, explicit null неизвестных trace metadata и deprecated reasoning-ms как wall time; не переименовывать его смысл в measured reasoning.                                |
| `cli.rs::Response/Availability`, `cli-protocol.rs::ReasoningBlock` | `answer`, `trace`; `provider`, `available`, `path`; `id`, `kind`, `text`                                                                  | Имена уже совместимы. Файловый путь availability, summary и block text — значения, а не ключи для нормализации.                                                                                                                                                |

Keychain service `dev.proteus-int.social` и имена `<provider>-config`, `<provider>-token`, `github-pending`, `vk-pending`, `corpus-key-v1`, `telegram-database-key` — стабильные адреса хранения. Они не переименовываются ради стиля. Последние два содержат raw base64 ключи, а не JSON DTO: их нельзя пропускать через JSON codec. Ключ TDLib и база `sources/telegram` принадлежат нативному адаптеру; format/schema TDLib не является собственной схемой PROTEUS.

`EncryptedStore` использует header `PRT1`, nonce 12 bytes и AAD `PROTEUS-CORPUS-V1`; лимиты текущего writer/read — 32 MiB plaintext и 33 MiB file. Эти признаки и `corpus-key-v1` сохраняются для чтения legacy. Проверка новой SQLCipher записи включает всех документов, grants, checkpoints/imports, связей и hashes; commit generation только после верификации. Отказ Keychain, decrypt, размера, версии или диска оставляет оригинал доступным. Новый secret envelope и storage generation вводятся через отдельные codecs; не переключать global `rename_all` существующего struct без его legacy reader.

### Внешние и непрозрачные исключения

- VK/GitHub/Instagram HTTP payloads и query/form: `client_id`, `access_token`, `refresh_token`, `device_code`, `user_code`, `expires_in`, `grant_type`, `redirect_uri`, `code_verifier`, `code_challenge`, `code_challenge_method`, `device_id`, `user_id`, provider `error`/`response` и остальные документированные внешние поля. Собственный Token хранит их под своей схемой после проверки provider adapter.
- `social-oauth.rs::VkCallback` — внутренний Rust parse-result с `device_id`; входной callback query сохраняет внешние `device_id`, `state`, `code`, `error` и существующий URL-протокол. Нельзя менять query имена вместе с serde DTO.
- TDLib JSON `@type`, `@extra`, `database_directory`, `database_encryption_key`, `api_id`, `api_hash`, `other_user_ids`, `phone_number` и все остальные SDK keys остаются внешними. Plain `serde_json::Value` в очередях и provider responses не даёт разрешения на rename.
- `RawItem` и `Page` в `sources.rs` не derive Serialize/Deserialize: это внутренние Rust структуры (`author_label`, `reply_to`, `not_analyzed` и т. д.) с обычным snake_case. Их нельзя включать в список wire миграций только по имени поля.
- Source hashes вычисляются из прежнего точного текста и ID-связей; schema migration не делает повторный fetch, модельный запрос, deanonymization или text normalization. Внешние Codex/Kimi stream events и Kimi config schema сохраняют свои имена независимо от собственного CLI Request.

## Проверяемое соответствие известных ключей

Таблица снята через TypeScript AST PropertySignature/PropertyAssignment, не поиском дефисов во всех строках. Перечень показывает места определения/создания ключа, а не разрешает универсальный runtime rename. Keys словарей ComponentId сохраняются как ID. Для каждого формата применяются только поля его схемы из таблицы. Rust symbols остаются snake_case, serde wire соответствует camelCase столбцу.

| Legacy key                          | Canonical key / исключение         | Определение или writer                                                                                                                                                                                                                           |
| ----------------------------------- | ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `account-id`                        | `accountId`                        | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                                                                   |
| `active-reasoning-ms`               | `activeReasoningMs`                | `apps/desktop/src/shared/lib/cli.ts`, `packages/contracts/src/index.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                                   |
| `allow-model-processing`            | `allowModelProcessing`             | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                                                                   |
| `analyzed-documents`                | `analyzedDocuments`                | `packages/stalker/src/index.ts`, `packages/stalker/src/source-analysis.ts`                                                                                                                                                                       |
| `answer-text-may-be-redacted`       | `answerTextMayBeRedacted`          | `packages/pipeline/src/export.ts`                                                                                                                                                                                                                |
| `api-hash`                          | `apiHash`                          | `packages/contracts/src/connections.ts`                                                                                                                                                                                                          |
| `api-id`                            | `apiId`                            | `packages/contracts/src/connections.ts`                                                                                                                                                                                                          |
| `applied-effort`                    | `appliedEffort`                    | `apps/desktop/src/shared/lib/cli.ts`, `packages/contracts/src/index.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                                   |
| `author-id`                         | `authorId`                         | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-model-document.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                  |
| `author-label`                      | `authorLabel`                      | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-model-document.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                  |
| `base-profile`                      | `baseProfile`                      | `packages/pipeline/src/export.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                                                                         |
| `baseline-difference`               | `baselineDifference`               | `packages/pipeline/src/experiment.ts`                                                                                                                                                                                                            |
| `behavior-summary`                  | `behaviorSummary`                  | `packages/stalker/src/index.ts`, `packages/stalker/src/source-analysis.ts`                                                                                                                                                                       |
| `brain-analyzator`                  | `без изменения: ID / artifact map` | `packages/intelligence/src/model-settings.ts`                                                                                                                                                                                                    |
| `cli-version`                       | `cliVersion`                       | `apps/desktop/src/shared/lib/cli.ts`, `packages/contracts/src/index.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                                   |
| `client-id`                         | `clientId`                         | `packages/contracts/src/connections.ts`                                                                                                                                                                                                          |
| `comparison-eligible`               | `comparisonEligible`               | `packages/brain-scoring/src/evaluation.ts`, `packages/contracts/src/evaluation.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                        |
| `content-hash`                      | `contentHash`                      | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                                                                   |
| `context-modes`                     | `contextModes`                     | `packages/person-fabric/src/style.ts`, `packages/contracts/src/style.ts`                                                                                                                                                                         |
| `created-at`                        | `createdAt`                        | `packages/contracts/src/experiment.ts`, `packages/contracts/src/sources.ts`, `packages/stalker/src/source-model-document.ts`, `packages/stalker/src/source-fixtures.ts`, `packages/pipeline/src/experiment.ts`                                   |
| `criterion-id`                      | `criterionId`                      | `packages/brain-scoring/src/evaluation.ts`, `packages/brain-analyzator/src/index.ts`, `packages/contracts/src/evaluation.ts`                                                                                                                     |
| `document-count`                    | `documentCount`                    | `packages/contracts/src/connections.ts`                                                                                                                                                                                                          |
| `document-id`                       | `documentId`                       | `packages/contracts/src/style.ts`, `packages/stalker/src/source-analysis.ts`                                                                                                                                                                     |
| `document-ids`                      | `documentIds`                      | `packages/contracts/src/persona.ts`, `packages/contracts/src/sources.ts`, `packages/stalker/src/source-analysis.ts`                                                                                                                              |
| `duration-ms`                       | `durationMs`                       | `packages/stalker/src/source-controller.ts`                                                                                                                                                                                                      |
| `elapsed-ms`                        | `elapsedMs`                        | `apps/desktop/src/shared/lib/cli.ts`                                                                                                                                                                                                             |
| `estimated-bsc`                     | `estimatedBsc`                     | `packages/brain-scoring/src/index.ts`                                                                                                                                                                                                            |
| `evidence-ids`                      | `evidenceIds`                      | `packages/person-fabric/src/index.ts`, `packages/contracts/src/persona.ts`                                                                                                                                                                       |
| `exemplar-ids`                      | `exemplarIds`                      | `packages/person-fabric/src/style.ts`, `packages/contracts/src/style.ts`                                                                                                                                                                         |
| `expires-at`                        | `expiresAt`                        | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                                                                   |
| `export-allowed`                    | `exportAllowed`                    | `packages/contracts/src/style.ts`, `packages/stalker/src/source-analysis.ts`                                                                                                                                                                     |
| `export-metadata`                   | `exportMetadata`                   | `packages/pipeline/src/export.ts`                                                                                                                                                                                                                |
| `finished-at`                       | `finishedAt`                       | `packages/pipeline/src/experiment.ts`                                                                                                                                                                                                            |
| `format-attempt`                    | `formatAttempt`                    | `packages/contracts/src/index.ts`, `packages/intelligence/src/structured.ts`                                                                                                                                                                     |
| `format-attempts`                   | `formatAttempts`                   | `packages/contracts/src/experiment.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                                                                    |
| `format-rules`                      | `formatRules`                      | `packages/person-fabric/src/style.ts`, `packages/contracts/src/style.ts`                                                                                                                                                                         |
| `global-model`                      | `globalModel`                      | `packages/intelligence/src/model-settings.ts`                                                                                                                                                                                                    |
| `is-forwarded`                      | `isForwarded`                      | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-model-document.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                  |
| `judge-disagreement`                | `judgeDisagreement`                | `packages/brain-scoring/src/evaluation.ts`, `packages/contracts/src/evaluation.ts`                                                                                                                                                               |
| `known-document-ids`                | `knownDocumentIds`                 | `packages/stalker/src/source-controller.ts`                                                                                                                                                                                                      |
| `max-bytes`                         | `maxBytes`                         | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                                                                   |
| `max-documents`                     | `maxDocuments`                     | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                                                                   |
| `max-duration-ms`                   | `maxDurationMs`                    | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                                                                   |
| `max-steps`                         | `maxSteps`                         | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                                                                   |
| `model-settings`                    | `modelSettings`                    | `packages/contracts/src/experiment.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                                                                    |
| `not-analyzed`                      | `notAnalyzed`                      | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-analysis.ts`, `packages/stalker/src/source-controller.ts`, `packages/stalker/src/source-fixtures.ts`                                                                           |
| `omitted-source-excerpts`           | `omittedSourceExcerpts`            | `packages/pipeline/src/export.ts`                                                                                                                                                                                                                |
| `omitted-style-exemplars`           | `omittedStyleExemplars`            | `packages/pipeline/src/export.ts`                                                                                                                                                                                                                |
| `participant-order`                 | `participantOrder`                 | `packages/contracts/src/experiment.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                                                                    |
| `person-fabric`                     | `без изменения: ID / artifact map` | `packages/intelligence/src/model-settings.ts`                                                                                                                                                                                                    |
| `person-id`                         | `personId`                         | `packages/brain-scoring/src/index.ts`, `packages/brain-scoring/src/evaluation.ts`, `packages/brain-analyzator/src/index.ts`, `packages/contracts/src/index.ts`, `packages/contracts/src/evaluation.ts`, `packages/pipeline/src/experiment.ts`    |
| `persona-label`                     | `personaLabel`                     | `packages/person-fabric/src/index.ts`, `packages/person/src/index.ts`                                                                                                                                                                            |
| `poll-after-ms`                     | `pollAfterMs`                      | `packages/contracts/src/connections.ts`                                                                                                                                                                                                          |
| `preparation-reasoning-omitted`     | `preparationReasoningOmitted`      | `packages/pipeline/src/export.ts`                                                                                                                                                                                                                |
| `preparation-wall-time-ms`          | `preparationWallTimeMs`            | `packages/pipeline/src/experiment.ts`                                                                                                                                                                                                            |
| `prompt-version`                    | `promptVersion`                    | `packages/contracts/src/experiment.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                                                                    |
| `public-urls`                       | `publicUrls`                       | `packages/contracts/src/index.ts`, `packages/stalker/src/index.ts`, `packages/stalker/src/source-fixtures.ts`, `packages/pipeline/src/index.ts`                                                                                                  |
| `qr-code`                           | `qrCode`                           | `packages/contracts/src/connections.ts`                                                                                                                                                                                                          |
| `reasoning-minutes`                 | `reasoningMinutes`                 | `packages/brain-scoring/src/index.ts`, `packages/contracts/src/index.ts`                                                                                                                                                                         |
| `reasoning-ms`                      | `reasoningMs`                      | `apps/desktop/src/shared/lib/cli.ts`, `packages/brain-scoring/src/index.ts`, `packages/contracts/src/index.ts`, `packages/pipeline/src/experiment.ts`                                                                                            |
| `reasoning-patterns`                | `reasoningPatterns`                | `packages/person-fabric/src/style.ts`, `packages/contracts/src/style.ts`                                                                                                                                                                         |
| `reasoning-truncated`               | `reasoningTruncated`               | `apps/desktop/src/shared/lib/cli.ts`, `packages/contracts/src/index.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                                   |
| `reply-to`                          | `replyTo`                          | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-model-document.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                  |
| `requested-effort`                  | `requestedEffort`                  | `apps/desktop/src/shared/lib/cli.ts`, `packages/contracts/src/index.ts`, `packages/contracts/src/experiment.ts`, `packages/pipeline/src/experiment.ts`                                                                                           |
| `requested-model`                   | `requestedModel`                   | `apps/desktop/src/shared/lib/cli.ts`, `packages/contracts/src/index.ts`, `packages/contracts/src/experiment.ts`, `packages/pipeline/src/experiment.ts`                                                                                           |
| `required-status`                   | `requiredStatus`                   | `packages/brain-scoring/src/evaluation.ts`, `packages/contracts/src/evaluation.ts`                                                                                                                                                               |
| `resolved-model`                    | `resolvedModel`                    | `apps/desktop/src/shared/lib/cli.ts`, `packages/contracts/src/index.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                                   |
| `resource-id`                       | `resourceId`                       | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-model-document.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                  |
| `retrieved-at`                      | `retrievedAt`                      | `packages/contracts/src/persona.ts`, `packages/contracts/src/sources.ts`, `packages/stalker/src/index.ts`, `packages/stalker/src/source-analysis.ts`, `packages/stalker/src/source-fixtures.ts`                                                  |
| `schema-version`                    | `schemaVersion`                    | `packages/pipeline/src/experiment.ts`                                                                                                                                                                                                            |
| `scores-reference-original-answers` | `scoresReferenceOriginalAnswers`   | `packages/pipeline/src/export.ts`                                                                                                                                                                                                                |
| `scoring-version`                   | `scoringVersion`                   | `packages/pipeline/src/experiment.ts`                                                                                                                                                                                                            |
| `snapshot-id`                       | `snapshotId`                       | `packages/contracts/src/sources.ts`, `packages/stalker/src/index.ts`, `packages/stalker/src/source-analysis.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                       |
| `solution-wall-time-ms`             | `solutionWallTimeMs`               | `packages/pipeline/src/experiment.ts`                                                                                                                                                                                                            |
| `source-context`                    | `sourceContext`                    | `packages/person-fabric/src/index.ts`, `packages/contracts/src/index.ts`, `packages/contracts/src/persona.ts`, `packages/intelligence/src/structured.ts`, `packages/pipeline/src/experiment.ts`                                                  |
| `source-coverage`                   | `sourceCoverage`                   | `packages/stalker/src/index.ts`, `packages/stalker/src/source-analysis.ts`                                                                                                                                                                       |
| `source-snapshots`                  | `sourceSnapshots`                  | `packages/contracts/src/experiment.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                                                                    |
| `source-status`                     | `sourceStatus`                     | `packages/person-fabric/src/index.ts`, `packages/contracts/src/persona.ts`, `packages/person/src/index.ts`                                                                                                                                       |
| `source-url`                        | `sourceUrl`                        | `packages/contracts/src/persona.ts`, `packages/contracts/src/sources.ts`, `packages/stalker/src/source-model-document.ts`, `packages/stalker/src/index.ts`, `packages/stalker/src/source-analysis.ts`, `packages/stalker/src/source-fixtures.ts` |
| `stage-timeout-ms`                  | `stageTimeoutMs`                   | `packages/contracts/src/experiment.ts`, `packages/pipeline/src/experiment.ts`                                                                                                                                                                    |
| `style-conflicts`                   | `styleConflicts`                   | `packages/contracts/src/style.ts`, `packages/person/src/index.ts`                                                                                                                                                                                |
| `style-exemplar-ids`                | `styleExemplarIds`                 | `packages/person-fabric/src/style.ts`, `packages/contracts/src/style.ts`                                                                                                                                                                         |
| `style-exemplars`                   | `styleExemplars`                   | `packages/person-fabric/src/index.ts`, `packages/contracts/src/persona.ts`, `packages/person/src/index.ts`, `packages/stalker/src/index.ts`, `packages/stalker/src/source-analysis.ts`, `packages/pipeline/src/export.ts`                        |
| `style-profile`                     | `styleProfile`                     | `packages/person-fabric/src/index.ts`, `packages/contracts/src/persona.ts`, `packages/person/src/index.ts`                                                                                                                                       |
| `system-prompt`                     | `systemPrompt`                     | `packages/contracts/src/index.ts`, `packages/intelligence/src/structured.ts`                                                                                                                                                                     |
| `task-skills`                       | `taskSkills`                       | `packages/person-fabric/src/index.ts`, `packages/contracts/src/persona.ts`, `packages/person/src/index.ts`                                                                                                                                       |
| `terminal-ms`                       | `terminalMs`                       | `apps/desktop/src/shared/lib/cli.ts`, `packages/brain-scoring/src/index.ts`, `packages/contracts/src/index.ts`, `packages/pipeline/src/experiment.ts`                                                                                            |
| `thinking-style`                    | `thinkingStyle`                    | `packages/stalker/src/index.ts`, `packages/stalker/src/source-analysis.ts`                                                                                                                                                                       |
| `total-documents`                   | `totalDocuments`                   | `packages/contracts/src/sources.ts`, `packages/stalker/src/source-controller.ts`, `packages/stalker/src/source-fixtures.ts`                                                                                                                      |
| `user-code`                         | `userCode`                         | `packages/contracts/src/connections.ts`                                                                                                                                                                                                          |
| `voice-rules`                       | `voiceRules`                       | `packages/person-fabric/src/style.ts`, `packages/contracts/src/style.ts`                                                                                                                                                                         |
| `wall-time-ms`                      | `wallTimeMs`                       | `apps/desktop/src/shared/lib/cli.ts`, `packages/contracts/src/index.ts`, `packages/contracts/src/experiment.ts`, `packages/pipeline/src/experiment.ts`                                                                                           |

## Отказы, точность и восстановление

- Decoder выбирает одну конкретную версию до преобразования. Unknown version, mixed legacy/canonical поля (даже одинаковые), конфликт aliases, неверные типы, неподдержанные поля и oversize отклоняются. Нельзя silently overwrite имя или вернуть defaults внутри миграции. UI может отдельно предложить восстановление.
- Undefined отсутствует в JSON; optional остаётся отсутствующим, explicit null сохраняется только там, где разрешён схемой. Не превращать неизвестного автора или неизвестную модель в пустую строку или выдуманное значение.
- Произвольные JSON maps, filenames, ComponentId/status/operation values, URL, имена, handles, сообщения, Markdown, CRLF/LF, Unicode, emoji, опечатки, hashes, timestamps и отношения не переименовываются/не нормализуются. External OAuth `client_id`, `redirect_uri`, VK/TDLib fields, Tauri commands/event IDs, HTML `aria-*`/`data-*` сохраняют внешний контракт.
- Новая запись готовится отдельно, проходит decode/roundtrip/count/hash/link verification, затем публикуется атомарно. При сбое остаётся оригинал и прежний active generation. Повтор миграции не дублирует документы. Restart читает только committed generation; temp/WAL остаются зашифрованными. Удалять прежние файлы до верификации запрещено.
- Порядок: inventory → boundary fixtures/codecs → атомарный TS switch со всеми consumers → Rust serde/writers+frontend → пакетные extraction и UX/RAG интеграция. Общие файлы мигрируются один раз, без доменных double aliases, any или as unknown as.
- Сначала fixture tests для legacy/new/collision/unknown/oversize/Unicode/opaque keys и crash recovery; затем consumer switch. Старые production parsers до switch работают как прежде. Synthetic fixture не использует реальные accounts или grants.

## Lifecycle extension результата schemaVersion 4

Новые результаты сохраняют optional `execution` с собственным `schemaVersion: 1`.
Отсутствие extension у исторического результата означает неизвестные metadata,
а не ноль повторов и не восстановленные задним числом timestamps. Decoder
`decodeExecutionSnapshot` отвергает смешанные ключи, неизвестную версию, повторные
IDs и некорректные состояния. Export проверяет extension перед записью.

`runId` относится к эксперименту, `stageId` — к логическому этапу, `callId` —
к одному обращению к модели. Отдельный `logicalStageId` сохраняет прежние ключи
этапов. Host передаёт `callId` как native request `id`; поле `invocationId`
не добавляется в Rust wire DTO. `formatRepair` отмечается только при второй
попытке формата, обычные source-plan/chunk-analysis вызовы повторами не считаются.

Состояния `running`, `validating`, `formatRetry`, `completed`, `failed`, `cancelled`
и `skipped` описывают наблюдаемое выполнение. Метаданные этапа сохраняются и при
ошибке до ответа CLI через callback `onExecution`; завершённый результат содержит
снимок всех начатых и явно пропущенных этапов. Старые `attempts` и агрегированные
traces сохранены для совместимости. Измеряется wall time; активное время
рассуждений по-прежнему неизвестно. Ответы не превращаются в reasoning.

## Native source binding и record generation 3

`create_source_binding` выдаёт сохранённую native-привязку персоны, роли источника,
выбранного ресурса, автора и явно разрешённых собеседников. `source_binding_action`
принимает только её ID и разрешённое действие. Native повторно проверяет область,
согласие, expiry, отмену и аккаунт после возможного I/O перед проекцией для модели.
Старый `source_action` не принимает grants с binding и не позволяет отбросить
атрибуцию. Неизвестный контекст требует отдельного явного разрешения.

Новый `proteus-source-record` version 3 хранится в `v3/<id>.enc`; grant payload
содержит обязательное `binding: null | SourceBinding`. Explicit readers v1/v2
сохраняются. Первый write не перезаписывает оригиналы v1/v2. Повреждённый v3
не вызывает fallback на старое разрешение. Отзыв/удаление сохраняют tombstone и
удаляют выбранные данные всех поколений; криптографический ключ и PRT1 AAD прежние.
В историческом срезе record generation 3 SQLCipher ещё не был внедрён; текущий writer описан ниже.

Bound import сохраняет исходные document IDs, reply links, authorship и labels.
Одинаковый текст разных документов не схлопывается. `sender_name` в архиве
Instagram — отображаемое имя, а не подтверждённый author ID: новые импорты хранят
его только как label, старые raw metadata остаются целыми, derived speaker остаётся
unknown. Legacy other/unknown не повышаются до own. Revision hash относится к
полному сохранённому документу даже при выдаче ограниченного фрагмента модели.

## Канонический SQLCipher-корпус и подготовленные снимки

Текущий `SourceStore` пишет в `sources/metadata.db` через `CorpusDatabase`.
SQLCipher 4 использует отдельный ключ `corpus-sqlcipher-key-v1` в системном
хранилище. Ключ `corpus-key-v1`, PRT1 и прежний AAD сохраняются только для
совместимости с encrypted files поколений v1/v2/v3. Чтение legacy проверяет
точную схему и переносит запись без изменения исходных файлов. Повреждённая
каноническая запись не разрешает fallback к прежнему разрешению.

Схема базы версии 5 добавила подготовленные снимки, эпизоды и чанки к
неизменяемым ревизиям, заданиям ingestion и поколенческому FTS. Миграция v4→v5
транзакционная: исходные records, revisions, snapshots, jobs и FTS остаются
неизменными. Конфликт DDL откатывает добавление таблиц и номер версии.

Текущая версия 6 добавляет таблицы поколений векторов, их blobs и указатель активного
поколения в ту же SQLCipher-базу. Миграция v5→v6 также транзакционная и не переписывает
прежние данные. Fixture `corpus-v5.sql` фиксирует схему до изменения; повторное
открытие идемпотентно. Новые поколения ссылаются на исходный snapshot и поколение FTS:
их удаление каскадно удаляет производные векторы. Активным может стать только
опубликованное поколение с manifest hash, опубликованные метаданные неизменяемы.
Это пока схема хранения: сценарии заполнения и чтения векторов с проверками grants
остаются отдельными обязательными частями tip.10–tip.11.

Подготовка создаёт отдельный снимок со ссылкой на исходный и фиксирует версии
разделения, токенизатора и chunking. Исходный снимок и receipt публикации не
перезаписываются. Повтор той же публикации возвращает прежний результат;
изменившийся состав требует нового publication ID. Эпизоды, чанки и receipt
записываются одной транзакцией; чанки ссылаются на точные UTF-8 диапазоны
оригинальных ревизий. Чтение ограничено количеством и байтами и проверяет
сохранённые hashes. Отзыв исходного снимка удаляет зависимые подготовки.

Это реализация хранения, а не завершённый пользовательский RAG-сценарий.
Native-подготовка подключена к IPC и использует закреплённый токенизатор из
ресурсов приложения. Runtime-поиск и интерфейс подготовки остаются условиями
приёмки tip.8–tip.15.
Хранилище не заменяет проверку действующих grants перед каждым доступом.

Одна исходная ревизия может иметь несколько разрешённых привязок Персон.
Подготовка сохраняет отдельные эпизоды и чанки каждой привязки, не размножая
оригинальные revisions в снимке. Цепочки ответов собираются внутри привязки;
изоляция holdout общая для всех копий оригинала. Повтор одной и той же пары
revision/binding и противоречащие друг другу метаданные оригинала отклоняются.
Порядок привязок не меняет fingerprint; дополнительная привязка не пересэмплирует
reference/holdout для тех же исходных ревизий. Формат хранения версии 5 не меняется.

## Идентичность диалогов Telegram Desktop JSON

Новый импорт использует adapter `telegramDesktopJson:2`, document schemaVersion 2
и batch codec `telegramArchiveBatch:2`. Экспорт Telegram содержит bare ID отдельно
от типа диалога: одинаковое число может относиться к пользователю, базовой группе
или каналу. Resource ID версии 2 включает эту область (`user | chat | channel`);
приватный и публичный варианты одного канала сохраняют общую идентичность.
Неизвестный тип получает отдельную область с hash точного типа без догадки о peer.
Исходный JSON, авторство, текст и внешние поля не переписываются.

Задания и preview версии 1 продолжают использовать прежние resource/source/document
IDs и batch codec при возобновлении, включая перезапуск приложения. Сохранённые
ревизии не переименовываются и не заменяются версиями 2. Неоднозначные bare IDs
старого preview по-прежнему отклоняются; автоматического объединения поколений нет.
Неизвестная версия адаптера или несовпадение версий job/preview отклоняются до
чтения исходного потока. Внешний IPC не позволяет выбирать legacy writer.

## Проверенная проекция Telegram для эпизодов

Native `ingestion::projection::project` получает входы эпизодов из завершённого
опубликованного задания. Перед чтением originals проверяются текущие grants;
отмена и expiry проверяются во время чтения, а права и исходный снимок —
повторно после разбора связей. Исходный acquisition deadline ограничивает
возобновление загрузки, но не чтение завершённого импорта при действующих правах.
Corpus-разрешение не заменяет отдельные model/export permissions.

Reader принимает точные сохранённые document v1/v2 и сверяет собственные поля
с повторной нормализацией raw JSON, provenance, версией адаптера, origin и ID.
Он не переименовывает внешние поля и не восстанавливает имя диалога из имени автора.
Отсутствующие обязательные nullable-поля, aliases и противоречащие оригиналу
text/author/time/reply-поля отклоняются. Hash ревизии по-прежнему проверяет хранилище.

Входы создаются для каждой разрешённой привязки без дублирования исходных revisions.
Внутренний conversation key ограничен стабильным hash пары origin/resource;
внешний ID и точный текст остаются в оригинале. Ответы учитывают типизированный
`reply_to_peer_id`; legacy bare ID не позволяет подменить peer другого класса.
Явные blockquote связываются со всеми точными совпадающими фрагментами разрешённых
оригиналов. Эти связи ограничивают reference/holdout, а не утверждают авторство
цитаты. Размер поиска, число patterns и рёбер ограничены; превышение даёт ошибку,
а не незаметное усечение связей.

Не найденные ответы/цитаты, пересылки без определимого оригинала и неподдержанная
разметка не объявляются полными. Неразмеченные произвольные частичные цитаты этим
адаптером пока не распознаются. Разбор известных полей соответствует
[официальному Telegram Desktop exporter](https://github.com/telegramdesktop/tdesktop/blob/fa588bb82443fb23d1b9f9fe9c6baa5bc9710f91/Telegram/SourceFiles/export/output/export_output_json.cpp).
Native `ingestion::preparation::prepare` связывает проекцию с подготовленной
публикацией. Проверка оригиналов возвращает неизменяемые входы без заимствования
соединения; токенизация выполняется вне общего database gate. Перед атомарной
записью повторно проверяются job, текущие grants и исходный снимок. Отмена,
истечение grant и отдельного бюджета подготовки прерывают работу; последняя
проверка внутри транзакции предотвращает частичную публикацию. Версия токенизатора
фиксируется на весь сценарий: её изменение отклоняется без записи результата.

Publication ID зависит от исходного снимка и версий projection/split/tokenizer/
chunking, но не от времени запуска или бюджета выполнения. Повтор после
перезапуска возвращает тот же подготовленный снимок; исходный job receipt и
loaded-only coverage не объявляют данные проиндексированными. Команда
`prepare_ingestion_job` принимает только versioned metadata и читает проверенный
E5-токенизатор из ресурсов приложения; UI и runtime retrieval ещё не завершены.

`SourceExcerpt.sourceMetadata` — необязательное расширение отображения источника
со своей `schemaVersion: 1`. Оно хранит формат, исходный object ID, заголовок,
метку ресурса, список имён авторов и ограничения. Отсутствие расширения сохраняет
совместимость ранее созданных фрагментов; это не миграция сохранённых оригиналов
и не новый источник разрешений. Новая TS-проекция заполняет только известные поля:
имя из `authorLabel`, формат/ID/ограничения из provenance, неизвестные заголовок
и метку ресурса оставляет `null`. Native-проекция страниц должна сохранить все
исходные author labels отдельными элементами, не выводя из них подтверждённый ID
человека. Контракт подготовлен; native-проекция и её IPC-проверка ещё подключаются.

`PersonaManifest` v2 добавляет обязательный `profileSnapshot` со своей версией 1:
точный task-free label/archetype/rules/style/exemplars и палитра. V1 читается без
добавления этого поля; null, несовместимая версия и несовпадающие exemplar IDs
отклоняются. Task adaptation, произвольные поля и разрешение на экспорт не входят
в этот новый body. TypeScript и Rust проверяют общий fixture с Unicode, CRLF,
BOM и NUL; строки не нормализуются. Лимит body — 512 KiB сериализованного UTF8.

Body хранится внутри существующего неизменяемого JSON манифеста, поэтому его
покрывают та же транзакция, суммарный бюджет, SHA256 записи и manifest pin.
SQL-миграция и переписывание V1 не нужны. Правила ссылаются на опубликованную
память; supported требует observed без противоречий. Образцы требуют точного
текста styleExample и связанного документа. Это проверки сохранённого пакета,
не новое разрешение на обработку и не доказательство истинности текста модели.
Fabric пишет draft/manifest V2 и сверяет полный body в квитанции перед возвратом
профиля. Person получает session V2 с полными сохранёнными навыками; старый
непривязанный V1 остаётся читаемым, но новый запуск требует повторной подготовки.
Перед резервированием CLI native сверяет все поля профиля, полные procedural
skills и выбранные memory item IDs. Произвольные поля и повторные JSON-ключи
отклоняются. Профиль остаётся task-free; условия задачи передаются отдельно.
Нейтральная роль фиксируется в body без персональных правил и образцов.

Версия `person-memory-profile:2` закрепляет общий TS/Rust текст из
`packages/contracts/src/persona-memory-prompt.json`. Системные инструкции нельзя
заменить через IPC. При второй попытке точная диагностика host validation передаётся
в ограниченном `formatFeedback` как данные; renderer добавляет фиксированное
указание исправить формат. Native проверяет попытку, размер и присутствие этого
поля. Это не доступ к скрытому reasoning. Живые grants, бюджет, cancellation,
source-context hash, no-tools CLI и доказательство исходного ответа перед записью
симуляции по-прежнему проверяются существующими владельцами. Подключение полной
подготовки/Person к основному pipeline и UI остаётся незавершённым.
