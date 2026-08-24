import type { Ref } from 'react';

export const NO_BENEFIT_DETAIL = 'The private candidate did not meet both requirements: at least 5% and 64 KiB smaller. No file was created, and your original stayed unchanged.';

interface NoBenefitResultProps {
  resultRef?: Ref<HTMLDivElement>;
}

export function NoBenefitResult({ resultRef }: NoBenefitResultProps) {
  return (
    <div ref={resultRef} className="no-benefit-result" tabIndex={resultRef ? -1 : undefined}>
      <strong>No worthwhile size reduction</strong>
      <span>{NO_BENEFIT_DETAIL}</span>
      <small>No output created</small>
    </div>
  );
}
