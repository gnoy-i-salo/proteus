# Проверки и рабочий цикл

## Команды

| Команда                               | Что проверяет                                                                                                            |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `pnpm check`                          | Формат всего репозитория, нейминг, LikeC4/FSD, тесты harness, ESLint, Stylelint, TypeScript, unit-тесты и frontend build |
| `pnpm check:package intelligence`     | Выбранный пакет и все транзитивно зависящие от него пакеты                                                               |
| `pnpm check:changed`                  | Изменения относительно `origin/trunk`, staged/unstaged и новые файлы                                                     |
| `pnpm check:changed HEAD~1`           | То же, с явной базой сравнения                                                                                           |
| `pnpm check:staged`                   | Staged-файлы; частично проиндексированные файлы отвергаются                                                              |
| `pnpm check:architecture`             | Модель LikeC4, совпадение manifests, границы пакетов и FSD                                                               |
| `pnpm test:harness`                   | Негативные примеры запрещённых импортов и селекция зависимых пакетов                                                     |
| `pnpm test -- packages/brain-scoring` | Только конкретные unit-тесты                                                                                             |
| `pnpm test:e2e`                       | Desktop-сценарии и production callback SPA: URL, handoff, ошибки, приватность и клавиатура в Chromium                    |
| `pnpm check:rust`                     | rustfmt, clippy с запретом предупреждений, Rust tests                                                                    |
| `pnpm build:desktop`                  | Нативный release-бинарник с frontend внутри                                                                              |

Селективный режим учитывает удалённые файлы и зависимые пакеты. Изменение корневых конфигураций, lockfile, архитектуры или harness запускает полный набор JavaScript-проверок. Изменение только документации не запускает сборки. Архитектура всегда проверяется целиком для изменений кода: это быстрая глобальная инварианта. Для изменённых нативных исходников/конфигурации селективный режим дополнительно вызывает Rust-проверки. Полный `pnpm check` намеренно переносим на машину без Rust; нативная проверка и e2e — отдельные обязательные CI jobs.

FSD проверяется для всех приложений в `apps/*`, включая `desktop` и `auth`. Селективная проверка запускает Stylelint и production build каждого затронутого frontend; изменение общих контрактов включает оба приложения. Негативные fixtures проверяют запрещённые FSD-импорты auth и невозможность пропустить его сборку.

Playwright сохраняет desktop-проект на `127.0.0.1:1420`. Проект `auth` каждый раз собирает SPA и запускает production preview на `127.0.0.1:1421/proteus/auth/`; существующий сервер для него не переиспользуется. Отдельный запуск: `pnpm exec playwright test --project=auth`. Проверяются прямое открытие и перезагрузка callback, удаление параметров URL, фиксированный native handoff, отсутствие внешних запросов и хранения ответа, ошибочные параметры и клавиатурное управление. В тестах используются только синтетические данные; переход в нативное приложение блокируется тестом. Реальная авторизация VK и поддержка callback в Rust этим не подтверждаются.

Первый запуск Rust-проверок требует собранного frontend: `pnpm build`. В CI frontend собирается до clippy/test. `Cargo.lock` включён в Git; CI работает с `--locked`.

Если Chrome уже установлен, можно выполнить `PLAYWRIGHT_CHANNEL=chrome pnpm test:e2e` без загрузки Chromium. CI и Release используют предустановленный Chrome на Ubuntu 24.04, чтобы не скачивать браузер и дополнительные шрифты при каждом запуске. Локально по умолчанию используется Chromium из Playwright.

## TDD и hooks

Перед реализацией поведения — падающий тест, затем минимальный код и зелёный тест. Для стартового каркаса так проверены настройки и расчёт BSC. Проверки harness дополнительно содержат запрещённые импорты и цикл, которые обязаны распознаваться как нарушения.

Для чистого перемещения модулей сначала зафиксировать наблюдаемое поведение существующими тестами, затем повторить их после переноса. Не придумывать падающий тест ради рефакторинга. Для миграции форматов и новых архитектурных запретов обязателен содержательный red/green, включая отрицательные fixtures. Отказ запуска инструмента не является red-фазой.

## Стандарт кода и переход

[Единый стандарт](code-style.md) заменяет прежнее правило kebab-case для собственных полей TypeScript и прежний CSS `--modifier`: собственные поля — camelCase, CSS Modules — `blockMore__elementMore_modificatorMaybe`, токены — camelCase. Пути и package IDs остаются kebab-case; Rust symbols — snake_case с camelCase на собственных сериализуемых границах. Старые форматы и внешние схемы обрабатываются у владельца, без массового переименования произвольного JSON и исходных цитат.

`check:naming` проверяет пути и package IDs, LikeC4/FSD — зависимости, а Stylelint — согласованную грамматику BEM и camelCase токенов. AST-проверки собственных полей, exports-only и UI imports вводятся отдельным срезом. Новые правила включаются с положительными и отрицательными fixtures; старые gates и запрет warnings сохраняются.

ESLint использует `recommendedTypeChecked` и `projectService` для всех TS/TSX приложений, пакетов, конфигураций и e2e. Ближайшие `tsconfig.json` задают настоящие проекты; обхода через `allowDefaultProject` нет. JS tooling сохраняет обычные правила без требования TS project. `no-floating-promises` проверяет также `void promise`: фоновой операции нужен обработчик ошибки у её владельца. Сохраняются SolidJS rules, unsafe checks и `no-misused-promises`. Основа конфигурации — официальные [typed linting instructions](https://typescript-eslint.io/getting-started/typed-linting/) и [projectService](https://typescript-eslint.io/packages/parser/#projectservice).

Ограничения: modified complexity ≤25 и max-depth ≤4 для TypeScript. По измеренному baseline 95-й процентиль сложности production-функций был 11, 99-й — 22; поэтому порог не требует искусственно дробить обычный JSX. Case branches учитываются как единый switch, вложенность проверяется отдельно. Отрицательные fixtures проверяют потерянные Promise, unsafe JSON, async callback вместо void и превышение обоих порогов. Единственное локальное исключение `prefer-promise-reject-errors` в protocol approval сохраняет разрешённый AbortSignal произвольный `reason`; две characterization-проверки закрепляют точную идентичность причины. Остальные правила не выключаются для тестов, adapters или целых каталогов.

Общие DTO переключаются одним согласованным срезом: сначала inventory и versioned codecs, затем все TS consumers, затем собственные Rust writers/IPC. Перемещение модулей выполняется отдельно. Перед изменением общего файла проверить актуальные задачи RAG, UX/UI и качества; владелец контракта один. Финальные `pnpm check`, `pnpm test:e2e`, `pnpm build`, `pnpm check:rust`, `pnpm build:desktop`, `pnpm build:pages` и `git diff --check` выполняются на итоговом общем дереве перед draft release. Четыре установщика должны происходить из одного source SHA.

Проектный навык находится в `.agents/skills/proteus-code-quality/SKILL.md`, применяется без глобальной установки и содержит примеры выбора владельца, сохранения реактивности и проверки совместимости. Форматирование: `pnpm exec prettier --write <явные-пути>` для затронутых JS/TS/SCSS/JSON/Markdown и `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml` для Rust. Не запускать массовое форматирование чужих изменений; `pnpm format` форматирует весь репозиторий.

## Hooks

Husky устанавливается через `pnpm install`:

- `pre-commit`: `pnpm check:staged`;
- `commit-msg`: Conventional Commits через commitlint;
- `pre-push`: `pnpm check`.

Частичный staging блокируется, чтобы проверенный рабочий файл не отличался от закоммиченного. Hooks не подменяют CI. GitHub Actions проверяет каждый commit сообщениями commitlint, весь JS-проект, e2e и нативные сборки macOS/Linux/Windows. Защита ветки и обязательность checks задаются в настройках GitHub отдельно; workflow сам не включает branch protection.

## Beads

Требуется `bd` 1.1.2 или совместимая версия с embedded Dolt. В этом репозитории инициализация выполнена без установки Beads hooks, чтобы не конфликтовать с Husky.

```sh
pnpm beads:init
bd prime
bd ready --json
bd show <id>
bd update <id> --status in_progress
# TDD → изменение → проверки → атомарный commit
bd close <id> --reason "Implemented and verified"
bd export -o .beads/issues.jsonl
```

После свежего клонирования локальная база отсутствует. Сначала выполнить `pnpm beads:init`; если история задач ещё не восстановлена из Dolt remote, импортировать переданный snapshot командой `bd import .beads/issues.jsonl`. Не импортировать snapshot поверх более свежей базы вслепую. `bd export`/`import` — перенос записей, не синхронизация истории Dolt. Настройки remote сохранены, но синхронизацию на другой машине следует проверить через `bd dolt --help`/`bd doctor` в установленной версии. [Документация Beads](https://github.com/steveyegge/beads).

## Подключения и TDLib

`pnpm prepare:tdlib` проверяет SHA-256 закреплённого архива до распаковки; negative fixture отвергает подменённый архив даже при существующем маркере. TDLib и OpenSSL собираются статически для целевой платформы. `SKILL.md` — точечное исключение для обязательного имени навыка; соседние файлы и каталоги продолжают проверяться на kebab-case.

Unit-тесты STALKER проверяют область grant, сроки, бюджеты, отмену, пагинацию, изоляцию holdout, точные выдержки и форматирование. Нативные тесты проверяют одноразовые OAuth-транзакции, ошибки API и локальную границу доступа; Playwright использует синтетические IPC-ответы для экранов подключений. Эти проверки не являются живым входом в пользовательские аккаунты.
