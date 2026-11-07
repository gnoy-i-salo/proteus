import { createSignal, createEffect, on } from 'solid-js';
import styles from './experiment-page.module.scss';

export function PublicLinks(props: {
  letter: string;
  revision: number;
  urls: string[];
  onChange: (urls: string[]) => void;
}) {
  const [index, setIndex] = createSignal(0);
  createEffect(
    on(
      () => props.revision,
      () => setIndex(0),
    ),
  );
  return (
    <div class={styles['experiment__links']}>
      <label>
        Публичные ссылки
        <input
          aria-label={`Публичная ссылка ${props.letter}`}
          type="url"
          placeholder="https://…"
          maxLength={2048}
          value={props.urls[index()] ?? ''}
          onInput={(event) =>
            props.onChange(
              props.urls.map((url, i) => (i === index() ? event.currentTarget.value : url)),
            )
          }
        />
      </label>
      <nav aria-label={`Ссылки ${props.letter}`}>
        <button
          type="button"
          aria-label={`Предыдущая ссылка ${props.letter}`}
          disabled={index() === 0}
          onClick={() => setIndex(index() - 1)}
        >
          ←
        </button>
        <span>
          {index() + 1} / {props.urls.length}
        </span>
        <button
          type="button"
          aria-label={`Следующая ссылка ${props.letter}`}
          disabled={index() >= props.urls.length - 1}
          onClick={() => setIndex(index() + 1)}
        >
          →
        </button>
        <button
          type="button"
          aria-label={`Добавить ссылку ${props.letter}`}
          title="Добавить ссылку (до 5)"
          disabled={props.urls.length >= 5}
          onClick={() => {
            props.onChange([...props.urls, '']);
            setIndex(props.urls.length - 1);
          }}
        >
          +
        </button>
        <button
          type="button"
          aria-label={`Удалить ссылку ${props.letter}`}
          title="Удалить ссылку"
          onClick={() => {
            const next = props.urls.filter((_, i) => i !== index());
            props.onChange(next.length ? next : ['']);
            setIndex(Math.max(0, index() - 1));
          }}
        >
          ×
        </button>
      </nav>
    </div>
  );
}
