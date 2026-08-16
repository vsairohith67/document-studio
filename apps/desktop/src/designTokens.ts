import tokens from '@document-studio/tokens';

type TokenLeaf = { value: string; type: string };
type TokenNode = TokenLeaf | { [key: string]: TokenNode };

function isLeaf(node: TokenNode): node is TokenLeaf {
  return typeof node === 'object' && 'value' in node && 'type' in node;
}

function getNode(path: string): TokenNode {
  let current: TokenNode = tokens as unknown as TokenNode;
  for (const segment of path.split('.')) {
    if (isLeaf(current) || !(segment in current)) {
      throw new Error(`Unknown design token: ${path}`);
    }
    current = current[segment];
  }
  return current;
}

function resolveValue(value: string, seen = new Set<string>()): string {
  const reference = value.match(/^\{([^}]+)\}$/)?.[1];
  if (!reference) {
    return value;
  }
  if (seen.has(reference)) {
    throw new Error(`Circular design token reference: ${reference}`);
  }
  seen.add(reference);
  const node = getNode(reference);
  if (!isLeaf(node)) {
    throw new Error(`Design token reference is not a value: ${reference}`);
  }
  return resolveValue(node.value, seen);
}

function collect(node: TokenNode, path: string[], output: Record<string, string>) {
  if (isLeaf(node)) {
    output[`--ds-${path.join('-').replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)}`] =
      resolveValue(node.value);
    return;
  }
  for (const [key, child] of Object.entries(node)) {
    if (key === '$schema') continue;
    collect(child, [...path, key], output);
  }
}

export function designTokenVariables(): Record<string, string> {
  const variables: Record<string, string> = {};
  collect(tokens as unknown as TokenNode, [], variables);
  return variables;
}

export function applyDesignTokens(root: HTMLElement = document.documentElement): void {
  for (const [name, value] of Object.entries(designTokenVariables())) {
    root.style.setProperty(name, value);
  }
}
