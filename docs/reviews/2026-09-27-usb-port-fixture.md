# Native USB listener fixture allocation (T655)

The Windows USB baseline failed before mutation testing: independent ephemeral
allocations could consume the video port's required `+2` sibling, and concurrent
fixtures repeatedly exhausted all 64 attempts. The permanent
`t655_concurrent_listener_fixtures_reserve_disjoint_pairs` regression failed
against the old helper on native Windows.

The helper now serializes each four-socket reservation and reserves the video
sibling before requesting an input port. The regression keeps eight fixtures
alive together across sixteen bounded rounds and checks all listener ports are
disjoint. It remains in the normal suite; production allocation is unchanged.

The unchanged final test source passed on native Windows as an ordinary user:
19 selected library tests and one portable USB integration test. The focused
Linux suite also passes all 16 USB tests. The first attempted fix still failed
the stress regression; the retained passing run includes the final reservation
lock. The reboot interrupted the subsequent Windows mutation campaign, so the
normal-suite pass is not presented as campaign completion.

[Evidence](artifacts/2026-09-27-mutation-resume/t655/) retains native red/green
logs, user/elevation result, Linux pass and the final source hash. The source
hash matches the fingerprinted Windows snapshot recovered after the reboot.
