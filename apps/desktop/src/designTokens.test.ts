import { describe, expect, it } from 'vitest';
import tokens from '@document-studio/tokens';
import { applyDesignTokens, designTokenVariables } from './designTokens';

describe('Document Studio design tokens', () => {
  it('resolves aliases from the committed token package', () => {
    const variables = designTokenVariables();

    expect(variables['--ds-core-color-accent']).toBe(tokens.core.color.accent.value);
    expect(variables['--ds-light-primary']).toBe(tokens.core.color.accent.value);
    expect(variables['--ds-core-radius-lg']).toBe(tokens.core.radius.lg.value);
  });

  it('applies the canonical variables to a DOM root', () => {
    const root = document.createElement('div');
    applyDesignTokens(root);

    expect(root.style.getPropertyValue('--ds-core-space-4')).toBe(tokens.core.space['4'].value);
    expect(root.style.getPropertyValue('--ds-light-primary')).toBe(tokens.core.color.accent.value);
  });
});
