# This block was not present in the `getbridgetype` script, and instead has
# been ported from `cafe.bat`, to remove a dependency on `cafe.bat`,
# `cafex_env.bat`, or `mochiato`.
#
# The original block was:
#
# ```batch
# :: Default hardware board connected to PC for [catdevmp|catdev4]
# if "%CAFE_HARDWARE%"=="" set CAFE_HARDWARE=catdevmp
# ```

function safe_substring($haystack, $start, $end = $null) {
	if ($haystack.Length -ge $start) {
		return $haystack.Substring($start, $end)
	} else {
		return ""
	}
}

if (-not $env:CAFE_HARDWARE) {
	$env:CAFE_HARDWARE = 'catdevmp'
}
$CH=$env:CAFE_HARDWARE

$CH_MP_CHECK = safe_substring -haystack "$CH" -start 6
$CH_CHAR_CHECK = safe_substring -haystack "$CH" -start 6 -end 1
if ($CH -eq "ev") {
	$env:BRIDGE_TYPE="Toucan"
} elseif ($CH -eq "ev_x4") {
	$env:BRIDGE_TYPE="Mion"
} elseif ($CH_MP_CHECK -eq "mp") {
	$env:BRIDGE_TYPE="Mion"
} elseif ($CH_CHAR_CHECK -le 2) {
	$env:BRIDGE_TYPE="Toucan"
} else {
	$env:BRIDGE_TYPE="Mion"
}
