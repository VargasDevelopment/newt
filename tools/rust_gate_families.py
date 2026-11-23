"""Family identities shared by the Rust runner and the ordinary gate planner."""

FAMILIES = ('foundation', 'zip_window', 'inflate', 'zip_container', 'css', 'xml_input',
            'hyphenator', 'font_layout', 'image_probe', 'xml_session', 'package_parsers',
            'page_renderer', 'png_decoder', 'app_policy', 'chapter_layout', 'ft_font',
            'jpeg_decoder', 'reader_atlas', 'reader_navigation', 'epub_admission')
ADAPTER_FAMILIES = FAMILIES[1:]
FOUNDATION_CAMPAIGNS = {
    'records': ('base', 'identity', 'catalog', 'admission'),
    'clock-frame': ('clock', 'frame', 'prefix'),
    'statistics': ('tracker', 'stats', 'sc', 'book-graph'),
    'persistence': ('persistence', 'external', 'service'),
}


def foundation_partition(registry, campaigns=FOUNDATION_CAMPAIGNS):
    """Whole control modules must belong to exactly one campaign."""
    assigned = [module for modules in campaigns.values() for module in modules]
    controls = [control for module in registry.values() for control in module]
    if len(assigned) != len(set(assigned)) or set(assigned) != set(registry):
        raise ValueError('foundation modules need exactly one campaign')
    if len(controls) != len(set(controls)):
        raise ValueError('foundation control registered more than once')
    return {name: tuple(control for module in modules for control in registry[module])
            for name, modules in campaigns.items()}


# Whole positive suites moved by the Lead's measured CI-budget ruling.
FULL_POSITIVE_SOURCES = {
    'ft_font': ('rust/core/src/ft_font.rs', 'rust/core/src/ft_font_tests.rs'),
    'inflate': ('rust/core/src/inflate.rs', 'rust/core/src/inflate'),
    'epub_admission': ('rust/core/src/admission.rs', 'rust/core/src/admission_work.rs',
                       'rust/core/src/canonical.rs', 'rust/core/src/canonical_semantic.rs'),
    'reader_atlas': ('rust/reader-atlas',),
}
SHARED_INPUTS = ('rust/check.py', 'rust/expected.py', 'tools/rust_gate_families.py', 'rust/toolchain.json',
                 'rust/reference.json', 'rust/tests/inflate/run_support.py', 'tools/rust-toolchain.py')


def owner(family):
    return 'rust:' + family


def positive_owner(family):
    return 'rust-positive:' + family


def positive_owners(path):
    return tuple(positive_owner(family) for family, roots in FULL_POSITIVE_SOURCES.items()
                 if any(path == root or path.startswith(root + '/') for root in roots))


def test_inputs(root):
    """Declared family roots; shared harness dependencies are followed by gate_tier."""
    rust = root / 'rust'
    inputs = {owner(name): {rust / f'{name}_gate.py',
                           *[path for path in (rust / 'tests' / name).rglob('*') if path.is_file()]}
              for name in ADAPTER_FAMILIES}
    inputs[owner('foundation')] = {rust / 'tests/corpus.py', rust / 'expected.py',
                                  *(rust / 'tests').glob('*_controls.py'),
                                  *(rust / 'tests').glob('foundation-*.zlib')}
    inputs['family_dispatch_test.py'] = {rust / 'tests/family_dispatch_test.py'}
    return inputs
