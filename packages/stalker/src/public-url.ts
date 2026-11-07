export function validatePublicUrl(raw: string): string {
  const url = new URL(raw);
  const host = url.hostname.toLowerCase();
  if (
    !['https:', 'http:'].includes(url.protocol) ||
    url.username ||
    url.password ||
    host === 'localhost' ||
    host.endsWith('.local') ||
    host.endsWith('.localhost') ||
    !host.includes('.') ||
    host.includes(':') ||
    /^(0|10|127|169\.254|192\.168|172\.(1[6-9]|2\d|3[01]))\./u.test(host) ||
    /^\d+\.\d+\.\d+\.\d+$/u.test(host)
  )
    throw new Error('Нужна общедоступная http(s) ссылка без пароля и локального адреса.');
  return url.href;
}
