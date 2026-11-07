# Единый стандарт кода PROTEUS

Стандарт принят для `proteus-tip`, `proteus-x1y` и `proteus-gx9`. Он заменяет прежние требования kebab-case для собственных полей и CSS `--modifier`. Старые данные и внешние протоколы сохраняют свою схему на явных границах. Миграция не разрешает изменение исходных цитат, имён, отношений, URL, whitespace, hashes и исторических результатов.

## Имена и исключения

| Область                                | Правило             | Пример                                         |
| -------------------------------------- | ------------------- | ---------------------------------------------- |
| Собственные TS поля/переменные/функции | camelCase           | `sourceStatus`, `evidenceIds`, `wallTimeMs`    |
| Типы и компоненты                      | PascalCase          | `PersonProfile`, `ConnectionsPanel`            |
| Файлы, каталоги, package IDs           | kebab-case          | `parse-rules.ts`, `@proteus-int/person-fabric` |
| Rust поля/функции                      | snake_case          | `wall_time_ms`                                 |
| Собственные новые Rust DTO             | camelCase wire      | `#[serde(rename_all = "camelCase")]`           |
| CSS Module класс                       | camelCase части BEM | `modelSettings__row_active`                    |
| Собственные CSS/SCSS токены            | camelCase           | `--personaTurquoise`, `$personaTurquoise`      |

`README.md`, `AGENTS.md`, `SKILL.md`, `Cargo.toml`, `Cargo.lock`, `PULL_REQUEST_TEMPLATE.md` и обязательные платформенные иконки — точечные исключения путей. `openai.yaml` — имя метаданных навыка, его ключи задаёт внешний формат.

Сохраняются CSS properties/pseudo-classes, `aria-*`, `data-*`, HTML/SDK атрибуты, Tauri identifiers, CLI flags, OAuth/provider payload keys и требуемые имена конфигураций. Такие поля принадлежат внешнему DTO/адаптеру, а не доменному интерфейсу. Внешняя схема не оправдывает kebab-поля во всём модуле.

Значения `qr-required`, `source-retrieved`, `PERSON_FABRIC`, scoring/prompt versions, storage keys, MIME и opaque IDs не переименовываются вместе с полями. Новое собственное symbol-like значение предпочтительно camelCase; смена стабильного значения требует отдельной совместимой миграции. Использовать dot notation для собственных полей; доступ к внешнему ключу может оставаться `payload['access_token']`.

## CSS Modules

Каждая часть соответствует `[a-z][a-zA-Z0-9]*`. Грамматика:

```text
block[__element][_modifier]
^[a-z][a-zA-Z0-9]*(?:__[a-z][a-zA-Z0-9]*)?(?:_[a-z][a-zA-Z0-9]*)?$
```

Допустимы `block`, `blockMore`, `blockMore_active`, `blockMore__elementMore`, `blockMore__elementMore_modificatorMaybe`. Недопустимы `model-settings`, `block__row-more`, `block--active`, `block___active`, `block__row__icon` и `block_active_more`.

Модификатор дополняет базовый класс:

```tsx
<div
  class={styles.modelSettings__row}
  classList={{ [styles.modelSettings__row_active]: active() }}
/>
```

Файл остаётся `model-settings.module.scss`. Дефисы в `--personaTurquoise` — обязательный синтаксис CSS, имя токена camelCase. Селекторы, ссылки TSX, e2e и Stylelint меняются вместе. Проверить коллизии Sass: `_` и `-` эквивалентны в именах переменных. Не заменять дефисы в строках или CSS property names.

## Владельцы и публичные API

`architecture/proteus.c4` остаётся источником зависимостей; manifests и проверки меняются с моделью. Между пакетами допустим только публичный `@proteus-int/<package>` API, без deep/relative cross-package imports и циклов. `index.ts` содержит явные `export`/`export type`, без реализации и side effects. Внутри пакета импорт идёт прямо к модулю владельца, без self-import через barrel. Локальные типы остаются с владельцем; `contracts` содержит межпакетные договорённости.

Целевые группы отражают ответственность, а не требование создать каждый файл заранее:

| Пакет              | Разделение                                                                                                                                               |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `contracts`        | `intelligence`, `persona`, `experiment`, `evaluation`, `sources`, `connections`, `auth-callback`, `style`; codecs рядом со схемой или в `compatibility/` |
| `intelligence`     | `provider`, `model-settings`, `structured-response`                                                                                                      |
| `person`           | `person-port`, `solve-task`, `parse-reply`, `persona-artifacts`                                                                                          |
| `person-fabric`    | `fabricate-person`, `adapt-person`, `neutral-profile`, `parse-rules`, `parse-style`                                                                      |
| `brain-analyzator` | `rubric/` для создания/парсинга; `answers/` для оценки/парсинга                                                                                          |
| `brain-scoring`    | `score-trial`, `summarize-scores`, связанные interval helpers, `validate-rubric`, `legacy-score`                                                         |
| `stalker`          | `public-research/`, `source-collection/`, `source-analysis/`, `report`; `skills/` остаются ресурсами                                                     |
| `pipeline`         | `experiment/`, `execution/`, `presets/`, `export/`, validation                                                                                           |

Малый пакет остаётся плоским. Длинный prompt находится рядом со сценарием, тест — с владельцем, общие синтетические fixtures — в `testing/`. Не нужны пустые слои, цепочки barrel, новый package на каждую папку или общий склад `utils/helpers/managers/types`. Связанные короткие операции могут оставаться вместе. Сначала проверка сохранения поведения, затем перенос; не менять одновременно prompt semantics, names и декомпозицию.

## SolidJS 1.9 и Rust

FSD: `app → pages → widgets → features → entities → shared`; нет upward/cross-slice imports, sliced layer доступен через `index.ts`. `app` и `shared` не разбиваются на slices. `ui` отображает и хранит локальное визуальное состояние, `model` владеет сценарием/командами/отменой, `lib` содержит чистые операции своего slice. Shared IPC adapters переводят wire на доменный контракт; они не становятся хранилищем всей предметной логики.

Вызывать `create*Model` в reactive owner, передавать реактивные props как getters, сохранять cleanup, stale-response guards, abort/unlisten. Derived state — вычисление или memo, effect — side effect. Не применять React hooks или SolidJS 2 API. Не переносить весь компонент в гигантский controller и не создавать глобальный store только ради выноса.

Tauri commands проверяют вход/контекст и вызывают сценарий; parsing, providers и storage имеют отдельных владельцев. Rust остаётся идиоматичным, без обязательной DI/trait/class и без глобального запрета `clone`/`dyn Trait`. Не держать lock через длительный внешний I/O, если сценарий не требует сериализации; сохранять timeout/cancel/child cleanup. Типизированные внутренние ошибки преобразуются в безопасный IPC ответ без секретов. Набор Clippy lints выбирается по реальным проблемам, не включается `pedantic` целиком.

## Совместимая миграция

1. Зафиксировать owner, поддерживаемые версии, reader/writer и mapping каждого поля: TS, IPC/events, Rust serde, model JSON/prompts/skills, `.proteus`, results, localStorage, encrypted source storage и новые RAG данные.
2. Добавить schema-aware decoder legacy → canonical camelCase и отдельный новый encoder. Сначала тесты old/new, null/optional, invalid/oversize, unknown version и conflicting aliases. Не применять recursive generic `kebabToCamel`, `as unknown as` вместо валидации или aliases в domain interfaces.
3. Не менять формат под прежним номером. В текущем baseline experiment result имеет `schema-version: 3`, scoring version 2, preset `.proteus` version 1. Для нового writer выбрать свободную версию по актуальному коду. Не обещать reader результатов, пока он не реализован.
4. Переключить TS contracts и всех consumers одним зелёным срезом без смысловой декомпозиции; временный legacy native wire нормализует TS adapter. Затем согласованно переключить Rust DTO, frontend boundary и writers. Storage migration атомарна/идемпотентна, исходник сохраняется до проверки нового состояния; сбой допускает повтор и recovery.
5. Декомпозировать пакеты, UI и native отдельно. Общие RAG/UX contracts, source picker, storage и MemoryPort реализуются один раз; соседние эпики не создают копии.

Смешанный old/new payload отклоняется на неоднозначной границе; конфликт aliases никогда не разрешается молча. Исторические scores/prompt metadata не пересчитываются. Тесты используют синтетические Unicode/whitespace/relationships/URL, не читают credentials и не вызывают настоящих провайдеров. Grants, доступ к тексту, model processing и export restrictions остаются независимыми.

## Проверка и статус внедрения

Документ вводит стандарт; он не утверждает, что все прежние файлы уже мигрированы. После единой миграции CSS Stylelint проверяет согласованный BEM и camelCase design tokens; положительные/отрицательные fixtures закреплены в `scripts/style-rules.test.mjs`. `scripts/check-naming.mjs` пока проверяет пути/package IDs, `architecture-rules.mjs` — imports/cycles/FSD. AST own-field/exports-only/UI-import guards вводятся отдельным последующим срезом после подготовленных кодеков и переносов. Ни blanket excludes, ни ослабление старых gates не допустимы. Обновить этот статус после фактического включения проверок.

Поведение/миграции/harness проверяются TDD; чистые переносы — characterization до и regression после. Команды и hooks описаны в [harness](harness.md). На итоговом общем дереве обязательны JS, e2e, Rust, native и Pages checks; визуальная приёмка — 640×600, 1024×768, 1280×800 и увеличение текста. Сохранить палитры, серый центр логотипа, пиксельные window controls, BSC, approval protocol, frozen profiles и точный source text. Навык помогает выбрать границы, но автоматические ограничения обеспечивает CI.
