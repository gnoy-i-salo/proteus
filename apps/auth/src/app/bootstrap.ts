export function startAuth(): void {
  let callbackSearch: string | null = window.location.search;
  const hasFragment = window.location.hash.length > 0;

  // Scrub before loading the UI, styles, fonts or images. Callback data stays in memory.
  try {
    window.history.replaceState(null, '', window.location.pathname);
  } catch {
    callbackSearch = null;
  }
  if (hasFragment) callbackSearch = null;

  void import('./mount')
    .then(({ mountAuthApp }) => {
      mountAuthApp(callbackSearch);
      callbackSearch = null;
    })
    .catch(() => {
      callbackSearch = null;
      const root = document.getElementById('root');
      if (root)
        root.textContent =
          'Не удалось открыть страницу подключения. Вернитесь в PROTEUS и начните подключение заново.';
    });
}
