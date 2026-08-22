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
rejected(lambda value: value.__setitem__('state', str(value['state']).replace('G04B2 — active implementation', 'G04B2 planning')), 'missing active G04B2 status')
rejected(lambda value: value['asset_manifest'].__setitem__('files', []), 'missing exact asset allow-list')
rejected(lambda value: value.__setitem__('state', str(value['state']) + '\nAll virtualizer items are visible'), 'overscan visibility claim')
rejected(lambda value: value.__setitem__('session', str(value['session']).replace('16_777_216', '16_000_000')), 'canvas constant drift')
rejected(lambda value: value.__setitem__('surface', str(value['surface']).replace('event.altKey', 'event.shiftKey')), 'missing Alt handler')
rejected(lambda value: value.__setitem__('viewer', str(value['viewer']).replace('candidateDocumentRef', 'candidateRef')), 'missing transactional owner')

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

validate_g04b2 = namespace['validate_g04b2_boundaries']
g04b2_baseline = namespace['G04B2_VALIDATION_INPUTS']


def rejected_g04b2(mutator, label: str) -> None:
    candidate = deepcopy(g04b2_baseline)
    mutator(candidate)
    try:
        validate_g04b2(candidate)
    except SystemExit:
        return
    raise AssertionError(f'negative G04B2 validator probe was accepted: {label}')


rejected_g04b2(lambda value: value.__setitem__('package', str(value['package']).replace('"pdfjs-dist": "6.2.108"', '"pdfjs-dist": "6.3.0"')), 'renderer version drift')
rejected_g04b2(lambda value: value.__setitem__('contracts', str(value['contracts']).replace('PDF_TO_IMAGES_MAX_TOTAL_PIXELS: u64 = 67_108_864', 'PDF_TO_IMAGES_MAX_TOTAL_PIXELS: u64 = 100_000_000')), 'aggregate budget drift')
rejected_g04b2(lambda value: value.__setitem__('renderer', str(value['renderer']) + '\ncanvas.toBlob(() => undefined);'), 'browser blob encoder')
rejected_g04b2(lambda value: value.__setitem__('ipc', str(value['ipc']).replace('InvokeBody::Raw(bytes)', 'InvokeBody::Json(bytes)')), 'non-raw pixel IPC')
rejected_g04b2(lambda value: value.__setitem__('backend', str(value['backend']).replace('metadata.nonce != page.ticket.nonce', 'false')), 'nonce authentication removed')
rejected_g04b2(lambda value: value.__setitem__('typed_contracts', str(value['typed_contracts']).replace('viewerGeneration: number;', 'viewerGeneration: number;\n  sourcePath: string;')), 'React source path exposure')
rejected_g04b2(lambda value: value.__setitem__('capability', str(value['capability']).replace('"dialog:allow-open"', '"dialog:allow-open", "http:default"')), 'HTTP capability expansion')

print('G03/G04B/G04B2 repository-validator negative probes passed (19 cases).')
