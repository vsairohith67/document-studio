from copy import deepcopy
import runpy


namespace = runpy.run_path('scripts/validate_repo.py')
validate = namespace['validate_g03_acceptance_consistency']
baseline = namespace['G03_VALIDATION_INPUTS']


def rejected(mutator, label: str) -> None:
    candidate = deepcopy(baseline)
    mutator(candidate)
    try:
        validate(candidate)
    except SystemExit:
        return
    raise AssertionError(f'negative validator probe was accepted: {label}')


rejected(lambda value: value.__setitem__('state', str(value['state']) + '\nG03 READY TO STAGE'), 'stale ready state')
rejected(lambda value: value.__setitem__('state', str(value['state']) + '\nG03 is not complete'), 'stale incomplete status')
rejected(lambda value: value.__setitem__('state', str(value['state']) + '\nG04 remains blocked'), 'stale G04 block')
rejected(lambda value: value.__setitem__('state', str(value['state']).replace('G04A — active implementation', 'G04A planning')), 'missing active G04A status')
rejected(lambda value: value['asset_manifest'].__setitem__('files', []), 'missing exact asset allow-list')
rejected(lambda value: value.__setitem__('state', str(value['state']) + '\nAll virtualizer items are visible'), 'overscan visibility claim')
rejected(lambda value: value.__setitem__('session', str(value['session']).replace('16_777_216', '16_000_000')), 'canvas constant drift')
rejected(lambda value: value.__setitem__('surface', str(value['surface']).replace('event.altKey', 'event.shiftKey')), 'missing Alt handler')
rejected(lambda value: value.__setitem__('viewer', str(value['viewer']).replace('candidateDocumentRef', 'candidateRef')), 'missing transactional owner')

print('G03 repository-validator negative probes passed (9 cases).')
