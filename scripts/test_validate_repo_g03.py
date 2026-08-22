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
rejected(lambda value: value.__setitem__('state', str(value['state']).replace('G04B — active implementation', 'G04B planning')), 'missing active G04B status')
rejected(lambda value: value['asset_manifest'].__setitem__('files', []), 'missing exact asset allow-list')
rejected(lambda value: value.__setitem__('state', str(value['state']) + '\nAll virtualizer items are visible'), 'overscan visibility claim')
rejected(lambda value: value.__setitem__('session', str(value['session']).replace('16_777_216', '16_000_000')), 'canvas constant drift')
rejected(lambda value: value.__setitem__('surface', str(value['surface']).replace('event.altKey', 'event.shiftKey')), 'missing Alt handler')
rejected(lambda value: value.__setitem__('viewer', str(value['viewer']).replace('candidateDocumentRef', 'candidateRef')), 'missing transactional owner')
rejected(lambda value: value.__setitem__('session', str(value['session']).replace('validatePdfPageCount(document.numPages);', '')), 'missing early page-count admission')
rejected(lambda value: value.__setitem__('rust_pdf', str(value['rust_pdf']).replace('.try_reserve_exact(page_count)', '.reserve_exact(page_count)')), 'infallible page-sized allocation')

validate_g04b = namespace['validate_g04b_boundaries']
g04b_baseline = namespace['G04B_VALIDATION_INPUTS']


def rejected_g04b(mutator, label: str) -> None:
    candidate = deepcopy(g04b_baseline)
    mutator(candidate)
    try:
        validate_g04b(candidate)
    except SystemExit:
        return
    raise AssertionError(f'negative G04B validator probe was accepted: {label}')


rejected_g04b(lambda value: value.__setitem__('cargo', str(value['cargo']).replace('image = { version = "=0.25.10", default-features = false', 'image = { version = "=0.25.10", default-features = true')), 'wide codec features')
rejected_g04b(lambda value: value.__setitem__('contracts', str(value['contracts']).replace('IMAGE_MAX_DIMENSION: u32 = 8_192', 'IMAGE_MAX_DIMENSION: u32 = 16_384')), 'dimension drift')
rejected_g04b(lambda value: value.__setitem__('writer', str(value['writer']).replace('source_hashes', 'unchecked_sources')), 'source proof removed')
rejected_g04b(lambda value: value.__setitem__('rust_production', str(value['rust_production']) + '\npdf.to-images'), 'renderer path introduced')

print('G03/G04B repository-validator negative probes passed (15 cases).')
