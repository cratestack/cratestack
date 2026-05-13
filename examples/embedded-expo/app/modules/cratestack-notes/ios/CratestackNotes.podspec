Pod::Spec.new do |s|
  s.name           = 'CratestackNotes'
  s.version        = '0.1.0'
  s.summary        = 'cratestack JSON FFI dispatch bridged to React Native via Expo.'
  s.description    = 'Loads the embedded_expo_native staticlib (built via cargo) and exposes initDatabase / dispatch over the Expo modules API.'
  s.author         = 'cratestack'
  s.homepage       = 'https://github.com/cratestack/cratestack'
  s.platforms      = { :ios => '15.1' }
  s.source         = { git: '' }
  s.static_framework = true

  s.dependency 'ExpoModulesCore'

  s.pod_target_xcconfig = {
    'DEFINES_MODULE' => 'YES',
    # `SCDynamicStoreCopyComputerName` from `whoami` (transitive via the
    # cratestack facade → sqlx-postgres). The staticlib doesn't propagate
    # its rustc-link-lib lines through Xcode's final link, so we name the
    # framework here.
    'OTHER_LDFLAGS' => '-framework SystemConfiguration -framework CoreFoundation',
  }
  s.frameworks = 'SystemConfiguration', 'CoreFoundation'

  # The Rust staticlib lives in the cratestack workspace's target dir,
  # not under this module. Vendored as a static lib so CocoaPods links it
  # into the Expo app at build time. The pre-build script below shells out
  # to cargo so a clean checkout produces the .a on demand.
  s.vendored_libraries = '../../../../../target/aarch64-apple-ios-sim/release/libembedded_expo_native.a'

  s.script_phase = {
    :name => 'Build embedded_expo_native (cargo)',
    :script => <<~SCRIPT,
      set -euo pipefail
      cd "${PODS_TARGET_SRCROOT}/../../../../../../examples/embedded-expo/native"
      "$HOME/.cargo/bin/cargo" build --release --target aarch64-apple-ios-sim
    SCRIPT
    :execution_position => :before_compile,
  }

  s.source_files = "**/*.{h,m,mm,swift,hpp,cpp}"
end
