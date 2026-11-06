/// <reference types="vite/client" />
import type { SourceProvider } from '@proteus-int/contracts';
import telegram from '../skills/telegram-research/SKILL.md?raw';
import vk from '../skills/vk-research/SKILL.md?raw';
import instagram from '../skills/instagram-research/SKILL.md?raw';
import github from '../skills/github-research/SKILL.md?raw';

const skills: Readonly<Record<SourceProvider, string>> = { telegram, vk, instagram, github };

export function sourceSkill(provider: SourceProvider): string {
  const skill = skills[provider];
  if (!skill) throw new Error('STALKER: unsupported source provider');
  return skill;
}
