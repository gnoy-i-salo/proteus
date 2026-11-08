# Локальный токенизатор E5

`e5-tokenizer.rs` читает только явно переданный локальный файл. Он не скачивает
модель, не вызывает сеть и не выполняет embedding inference. Это подготовительный
компонент tip.8/tip.10; выбор embedding runtime, RU dialogue benchmark и индекс
остаются отдельными условиями приёмки.

Закреплён кандидат [intfloat/multilingual-e5-small](https://huggingface.co/intfloat/multilingual-e5-small/tree/614241f622f53c4eeff9890bdc4f31cfecc418b3)
(MIT по upstream model card). Используется
[onnx/tokenizer.json](https://huggingface.co/intfloat/multilingual-e5-small/resolve/614241f622f53c4eeff9890bdc4f31cfecc418b3/onnx/tokenizer.json)
из commit `614241f622f53c4eeff9890bdc4f31cfecc418b3`:

- Размер: `17082730` байт.
- SHA256: `0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39`.
- Runtime токенизации: `tokenizers = 0.23.2`, без default features и HTTP, с `fancy-regex`.
- Для документов применяется точный префикс `passage: `, для запросов — `query: `;
  специальные токены включены в подсчёт. Ограничение model input: 512 токенов.
- Сохранённые настройки truncation/padding отключаются. Подсчёт для chunking
  возвращает полную длину; model input сверх окна отклоняется, а не обрезается.
- Fingerprint включает upstream commit, SHA256, runtime и версии templates/special
  tokens. Исходный текст и диапазоны корпуса не нормализуются этим адаптером.

Публичный loader принимает только закреплённый файл. Отсутствие файла, другая
длина, checksum или повреждённый JSON дают явную ошибку. Файлы модели не хранятся
в Git. Полноценный FTS-only режим при отсутствии embedding weights реализуется
владеющим runtime, а не подменой токенизатора подсчётом символов.

Проверка на самостоятельно скачанном публичном tokenizer file:

```sh
cargo run --locked --manifest-path apps/desktop/src-tauri/Cargo.toml \
  --example e5-tokenizer-check -- /absolute/path/to/tokenizer.json
```

Пример проверяет точные token IDs русских строк, многоязычный подсчёт и отказ
при превышении окна, затем печатает время загрузки и p50/p95 для 100 подсчётов
одного синтетического текста. Эти числа относятся только к токенизации и не
измеряют embedding latency, RAM модели или качество retrieval. Unit fixtures
используют маленький синтетический словарь; они не заменяют эту проверку E5.
