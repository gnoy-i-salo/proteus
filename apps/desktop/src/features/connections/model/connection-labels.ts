import type { ConnectionState } from '@proteus-int/contracts';

export const names = { telegram: 'Telegram', vk: 'VK', github: 'GitHub', instagram: 'Instagram' };
export const descriptions = {
  telegram:
    'Доступная история выбранного чата. Для входа нужны API ID и API hash вашего приложения Telegram.',
  vk: 'Посты и комментарии в пределах выданных прав. Доступ к личным сообщениям проверяется отдельно.',
  github:
    'Выбранные репозитории и обсуждения. Нужен Client ID GitHub App с включённым device flow.',
  instagram: 'Личный аккаунт: импорт экспорта. Официальный API: собственный Business / Creator.',
};
export const stateLabels: Record<ConnectionState, string> = {
  unconfigured: 'Требуется настройка',
  disconnected: 'Не подключено',
  pending: 'Ожидаем подтверждения',
  'qr-required': 'Отсканируйте QR-код в Telegram',
  'phone-required': 'Введите номер телефона',
  'code-required': 'Введите код Telegram',
  'password-required': 'Введите пароль Telegram',
  'email-required': 'Введите адрес электронной почты',
  'email-code-required': 'Введите код из письма',
  connected: 'Подключено',
  error: 'Не удалось подключиться',
  unavailable: 'Подключение недоступно',
};
export const capabilityLabels: Readonly<Record<string, string>> = {
  profile: 'профиль',
  posts: 'публикации',
  comments: 'комментарии',
  messages: 'сообщения',
  repositories: 'репозитории',
  'archive-import': 'импорт файлов',
};
export const challenges: Readonly<
  Partial<Record<ConnectionState, { label: string; type: string }>>
> = {
  'phone-required': { label: 'Номер телефона Telegram', type: 'tel' },
  'code-required': { label: 'Код Telegram', type: 'text' },
  'password-required': { label: 'Пароль Telegram', type: 'password' },
  'email-required': { label: 'Email Telegram', type: 'email' },
  'email-code-required': { label: 'Код из письма Telegram', type: 'text' },
};
