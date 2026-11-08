# Локальный CPU runtime E5

`local_embedding::runtime::E5Runtime` выполняет inference на CPU через Candle
0.11.1 (F32). Native-слой включает проверяемый кеш модели и версионированное
хранилище векторов. Подключение к MemoryPort, пользовательскому управлению моделью
и итоговый RU benchmark ещё нужны.
Сценарий, вызывающий адаптер, обязан отдельно проверить разрешение на обработку
текста моделью. Загрузка файла сама по себе такого разрешения не даёт.

Единственный pin — `scripts/e5-embedding-package.json`: multilingual-e5-small,
revision `614241f622f53c4eeff9890bdc4f31cfecc418b3`, 384 измерения. Перед чтением
тензоров проверяются точные размер и SHA256 конфигурации и весов. Токенизатор
использует тот же revision и собственную проверку checksum. Fingerprint включает
все pins, runtime, pooling/normalization и fingerprint токенизатора с версиями
query/passage templates. Несовпадение ожидаемого профиля отклоняется до inference.

`pnpm prepare:models` готовит токенизатор; `pnpm prepare:embedding-model` явно
получает публичные веса и config для native-тестов. Обе команды проверяют SHA256,
не заменяют повреждённый cache и не оставляют частичный пакет. Файлы находятся в
игнорируемом `.cache/models`; веса не входят в Git и пока не включаются в app bundle.
Приложение не скачивает их автоматически. Runtime читает только переданные локальные
пути. Отсутствующий/повреждённый пакет возвращает типизированную причину недоступности;
облачного fallback, случайных или нулевых заменителей векторов нет.

Каждая последовательность ограничена 512 токенами, batch — 8 элементами и 4096
**padded** токенами. Маска строится по длинам: CLS с ID 0 не является padding.
Pooling — masked mean с накоплением f64 и L2 normalization; выход проверяется на
размерность и конечность. Исходные строки корпуса не изменяются.

Минимальная заявленная RAM reservation — 1536 MiB. Это предварительный допуск
к выполнению, а не OS-ограничитель RSS; суммарный лимит нескольких runtime должен
обеспечивать владеющий сценарий. Cancellation, отзыв разрешения и deadline проверяются
при чтении файлов, между токенизацией, inference и pooling. Текущий Candle forward
не прерывается посередине; результат после истечения бюджета не возвращается.
Это ограничение необходимо учитывать до подключения пользовательских операций.

Изолированный feasibility probe на macOS arm64, CPU F32, 2 потока: загрузка около
3713 ms, batch четырёх коротких passage около 47 ms, четырёх query около 37 ms,
peak RSS около 1029 MiB. Четыре синтетических русских retrieval-примера дали top-1
4/4, нормы и независимость от padding проверены. Это одиночное наблюдение небольшого
прототипа, не p50/p95, не максимальная RAM и не гарантия качества retrieval. Оно
не заменяет измерения итогового приложения на всех целевых платформах.

Native unit-тесты используют реальные закреплённые веса: проверяют размерность,
норму, padding, mismatch профиля, бюджеты, отмену и повреждённые файлы. CI и Release
явно готовят пакет до `pnpm check:rust`; отрицательные harness fixtures запрещают
пропуск подготовки и рассогласование runtime/model revision. FTS остаётся независим
от embedding weights. Полная приёмка tip.10 остаётся открытой до готовности
интеграции, управления ресурсами и измерений.

Native `local_embedding::cache::install_verified_artifact` проверяет и устанавливает
один закреплённый файл из переданного reader в локальный кеш по SHA256. Он не
скачивает модель: транспорт остаётся ответственностью сценария управления моделью.
Чтение ограничено порциями 1 MiB, проверяются точная длина,
checksum и отмена, включая EOF при повторном использовании существующего файла.
Проверенный временный файл публикуется через hard link; одновременные установки
одного pin проверяют уже опубликованный файл. Повреждённый существующий кеш
сохраняется и явно отклоняется, каталоги, symlink и другие не-обычные файлы
не принимаются за модель. Ошибка чтения или отмена не удаляют прежний хороший файл.

Временные файлы удаляются после нормального завершения/ошибки; аварийное завершение
может оставить временный каталог, очистка которого относится к сценарию запуска.
Защита от процесса с теми же правами ОС не заявлена; crash-durability зависит от ОС,
синхронизация каталога выполняется best-effort на Unix. Эта функция ещё не является
EmbeddingPort или пользовательским экраном установки.

`local_embedding::manager` объединяет три закреплённых файла в пакет. Native
`runtime::cache_descriptor()` получает pins из тех же спецификаций, что и inference,
и проверяет общую revision токенизатора и весов. Просмотр кеша ничего не создаёт;
готовность требует проверки всех трёх файлов. Повреждённая часть явно отклоняется,
даже если другая часть отсутствует. Установка открывает reader только для
отсутствующего файла и держит не более одного reader; проверенные части остаются
для повтора после сбоя. Прогресс считает только проверенные файлы и байты. Отмена,
включая последний callback прогресса, не возвращает успешный результат. Пути
остаются внутри native-слоя; скачивание и IPC/UI этим модулем не реализованы.

`local_embedding::search::search` выполняет точный cosine scan независимого
семантического списка. Алгоритм проходит iterator ссылок на векторы и сохраняет
только до 1000 лучших результатов и множество проверенных IDs (до 100000).
Вариант `search_stream` принимает краткоживущие строки базы; хранением и памятью
входного вектора управляет reader. Размер отдельного вектора —
1..4096 значений; текущий E5 producer выдаёт 384. Нормы и скалярные произведения
накапливаются в f64. Разные fingerprints/размерности, нечисловые значения, нулевые
нормы, повторные IDs и превышение бюджета отклоняют всю операцию. Равные оценки
упорядочиваются по точному ID; отмена проверяется между кандидатами и перед возвратом.
Эта функция не загружает весь corpus и не фильтрует кандидатов совпадениями FTS.

Входные кандидаты уже должны быть ограничены текущими grants, persona, reference
и периодом в native host. Численный алгоритм не является разрешением на чтение;
после поиска host обязан повторно проверить доступ. Хранилище и чтение векторных
кандидатов описаны ниже; подключение к MemoryPort ещё не реализовано.

Воспроизводимый синтетический probe находится в
`apps/desktop/src-tauri/src/e5-retrieval-observations.rs`: 16 реплик, восемь запросов,
десять повторов. Есть близкие отвлекающие реплики, отрицание, похожие номера заказов
и русскоязычный запрос к англоязычной реплике. Он использует настоящие закреплённые
веса и точный cosine scan; печатает ранги ожидаемых документов, top-1, recall@3,
MRR, загрузку и время batch из восьми query вместе с поиском. Тест проверяет
повторяемость, но не задаёт желаемый порог качества: неудачные ранги тоже наблюдения.
Это небольшой контролируемый пример, не оценка полного корпуса или сходства Персоны.

```sh
RAYON_NUM_THREADS=2 cargo test --locked --manifest-path apps/desktop/src-tauri/Cargo.toml \
  synthetic_ru_retrieval_observations_use_real_pinned_vectors -- --nocapture --test-threads=1
```

Для RSS запускать уже собранный test executable под системным измерителем памяти,
чтобы не смешивать память компилятора и модели. Профиль тестов, число потоков, ОС,
число кандидатов и размер batch нужно сохранять вместе с результатами. p50/p95
здесь относятся к десяти целым query batches, а не к отдельному запросу; percentile
использует nearest-rank. Это dev/test профиль с оптимизированными Candle kernels,
а не latency установленного release-приложения.

Наблюдение 08.11.2026, macOS 26.6 arm64, `RAYON_NUM_THREADS=2`, test профиль:
fingerprint `e5Embedding:1:ce3c9eddb139e3fdec09a40bee4fa020b2daf24abac421137630f67050e9559b`.
Ранги `[1, 1, 1, 1, 1, 1, 1, 2]`: top-1 **7/8**, recall@3 **8/8**, MRR **0.9375**.
Русский запрос к английской реплике оказался на втором месте. Загрузка — 24965 ms,
два passage batches — 15118 ms, query+search batch p50 — 5607 ms, p95 — 5670 ms.
Суммарный test process — 96.5 s. Отдельный снимок процесса во время выполнения
показал RSS 889056 KiB (около 868 MiB); это **не peak RSS**. Системный измеритель
peak RSS не получил доступ к `kern.clockrate`, поэтому максимум памяти не заявлен.
Результаты dev/test не сопоставимы напрямую с предыдущим optimized feasibility
probe и не доказывают улучшение относительно FTS или другой модели.

`retrieval_ranking::fuse` объединяет независимые лексический и семантический списки
по reciprocal-rank fusion (`k=60`, ранги от 1), затем ограничивает число результатов
из одного документа. Допускается до 1000 кандидатов в каждом списке и до 100
результатов; одинаковый chunk объединяется один раз, конфликт его document ID
отклоняется. Равные оценки упорядочиваются по chunk ID. Пустой семантический список
сохраняет честный lexical-only режим. Параметр `k=60` — фиксированный baseline,
а не результат подбора на этом синтетическом probe. Функция не проверяет grants
и не расширяет эпизоды: оба списка уже должны быть разрешены host, который повторно
проверяет доступ перед выдачей исходного текста. Внешнее расширение эпизодов,
retrieval receipts и интеграция MemoryPort ещё нужны.

## Durable vector generation storage

Schema v6 stores vectors in the same encrypted corpus database. Native storage
entry points begin/resume an exact stage, append one finite nonzero f32 vector
with an explicit matching model fingerprint, then publish only when every chunk
in the pinned FTS generation is present. Replays compare exact little-endian
bytes, including signed zero. Source publication drift fails before mutation.

Publication streams rows in binary chunk-ID order, hashes a versioned length-
prefixed model/dimensions/ID/vector representation, seals the generation and
switches the active pointer in one transaction. It never copies the full vector
set. Cancellation checks run before work, for every publication row and before
commit. A replay of an older sealed receipt cannot move the active pointer back.
Encrypted reopen tests cover interrupted staging and exact receipt replay.

These storage primitives do not authorize model processing or provide retrieval.
The native caller must check current grants before embedding and through the
checkpoint callback. Host authorization, bounded abandoned-stage cleanup, model management and
worker/MemoryPort integration remain required release work.

## Scoped native reads

Lexical search, reference-chunk enumeration and exact vector search share one
SQL scope filter (snapshot/generation, persona, grants, bindings and period).
Reference enumeration has no lexical query: keyset pages select at most 100
metadata rows before fetching exact text within the remaining byte budget
(maximum 64 MiB/page, 4 MiB/chunk). Missing or oversized stored text is an error;
a next chunk that does not fit remains behind the returned cursor.

Vector reads pin the full active vector receipt and its current FTS source in a
read savepoint. Unfinished or stale source generations cannot serve semantic
hits. A complete scope is applied before ranking. The SQL query streams vectors
without sorting their blobs; deterministic cosine ordering and tie breaks belong
to the bounded ranker. One row buffer can be dropped after each callback; a
storage, profile, cancellation or candidate-budget error discards all partial
hits. At most 100000 candidate IDs and 1000 result references are retained.

These are native database primitives, not the production MemoryPort or a grant
issuer. Model availability, host-owned scope, live permission rechecks, hybrid
orchestration, bounded episode expansion and retrieval receipts still need their
own integration and acceptance checks.
