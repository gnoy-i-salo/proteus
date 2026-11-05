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
| `pnpm test:e2e`                       | Настройки, сохранение, палитра и минимальная ширина окна в Chromium                                                      |
| `pnpm check:rust`                     | rustfmt, clippy с запретом предупреждений, Rust tests                                                                    |
| `pnpm build:desktop`                  | Нативный release-бинарник с frontend внутри                                                                              |

Селективный режим учитывает удалённые файлы и зависимые пакеты. Изменение корневых конфигураций, lockfile, архитектуры или harness запускает полный набор JavaScript-проверок. Изменение только документации не запускает сборки. Архитектура всегда проверяется целиком для изменений кода: это быстрая глобальная инварианта. Для изменённых нативных исходников/конфигурации селективный режим дополнительно вызывает Rust-проверки. Полный `pnpm check` намеренно переносим на машину без Rust; нативная проверка и e2e — отдельные обязательные CI jobs.

Первый запуск Rust-проверок требует собранного frontend: `pnpm build`. В CI frontend собирается до clippy/test. `Cargo.lock` включён в Git; CI работает с `--locked`.

Если Chrome уже установлен, можно выполнить `PLAYWRIGHT_CHANNEL=chrome pnpm test:e2e` без загрузки Chromium. По умолчанию CI использует Chromium из Playwright.

## TDD и hooks

Перед реализацией поведения — падающий тест, затем минимальный код и зелёный тест. Для стартового каркаса так проверены настройки и расчёт BSC. Проверки harness дополнительно содержат запрещённые импорты и цикл, которые обязаны распознаваться как нарушения.

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
