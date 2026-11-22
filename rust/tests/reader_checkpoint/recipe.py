# SPDX-License-Identifier: MIT
"""Shared host crate graph builder for startup wiring tests."""

HOST = 'x86_64-unknown-linux-gnu'


def graph(run, ws, target, clock, board, require):
    run.cargo(ws, target, 'build', '--release', '--lib', '-p', 'newt-reader-ui',
              '-p', 'newt-native-storage', '-p', 'newt-device-ui', '-p', 'newt-library-current',
              '--features', 'newt-device-ui/dev-usb', '--target', HOST)
    deps = target / HOST / 'release/deps'
    libraries = clock.rlibs(deps, require)
    adapters = run.output / (target.name + '-adapters')
    adapter = board.adapter(run, ws, HOST, deps, libraries, output=adapters)
    linked = {name: libraries[name] for name in ('newt_core', 'newt_device_ui',
              'newt_native_storage', 'newt_phased_reader', 'newt_phased_sink',
              'newt_reader_ui', 'newt_target_expat', 'newt_reader_navigation', 'newt_sdk_heap')}
    linked.update(newt_c3_board=adapter,
                  newt_native_io=adapters / ('libnewt_native_io-' + HOST + '-abort.rlib'))
    return linked
