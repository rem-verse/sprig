# So there were two original `setbridge` scripts. `setbridge` as a cygwin
# script, and `setbridge.bat`.
#
# These scripts differ in two main ways:
#
# 1. If you try to run the shell script directly as opposed to the batch file,
#    the shell script yells at you saying it can only be 'source'd. The batch
#    script does not contain this check, and is in fact the only way it's
#    documented being used (as a script).
# 2. If you set the "SESSION_MANAGER" environment variable to "1", than the
#    shell script tries to spin up a whole bunch of other commands to
#    `SessionManagerUtil.exe`. As far as I can tell the batch script does not
#    touch this at all and really only tries to set the title. :(
#
# all this to say... this script acts very weird in the official install
# of the Cafe SDK, and in order to maintain the most amount of compatability
# BOTH our BASH, and PWSH functions will support being `call`'d, and run
# directly as a command. Even if the originals techincally kind of did and
# didn't?

if (-not $env:SPRIG_RUNNING_FROM_SOURCE) {
	$env:SPRIG_RUNNING_FROM_SOURCE=0
}

$SPRIG_RC=0
if (("$env:SPRIG_RUNNING_FROM_SOURCE" -eq 1) -And (Test-Path -Path "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\..\target\debug\setbridgeconfig.exe")) {
	& "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\..\target\debug\setbridgeconfig.exe" $args
	$SPRIG_RC=$LASTEXITCODE
} elseif (("$env:SPRIG_RUNNING_FROM_SOURCE" -eq 1) -And (Test-Path -Path "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\..\target\release\setbridgeconfig.exe")) {
	& "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\..\target\release\setbridgeconfig.exe" $args
	$SPRIG_RC=$LASTEXITCODE
} else {
	& setbridgeconfig.exe $args
	$SPRIG_RC=$LASTEXITCODE
}

if ($SPRIG_RC -ne 0) {
	exit 1
}

$SPRIG_DELETED_BRIDGE=0
$bridge_ipaddr=""
$bridge_name=""
$is_default=""
for ($idx=0; $idx -lt $args.Length; $idx++) {
	if (-not $args[$idx].StartsWith("-")) {
		if (-not $bridge_name) {
			$bridge_name=$args[$idx]
		} else {
			$bridge_ipaddr=$args[$idx]
		}
	} elseif ($args[$idx] -eq "-d") {
		$SPRIG_DELETED_BRIDGE=1
	} elseif ($args[$idx] -eq "-default") {
		$is_default=" (and default)"
	}
}

if ($SPRIG_DELETED_BRIDGE -eq 1) {
	# The original script didn't clear `BRIDGE_CURRENT_*` variables, or switch to
	# the default bridge.
	Write-Output "Cleared stored IP address for $bridge_name (current environment unchanged)."
} else {
	Write-Output "Setting hostbridge for current session${is_default} to ${bridge_name} ${bridge_ipaddr}."
	$env:BRIDGE_CURRENT_NAME="$bridge_name"
	$env:BRIDGE_CURRENT_IP_ADDRESS="$bridge_ipaddr"
}

if ($env:SESSION_MANAGER -eq 1) {
	Write-Output "!!! NOT YET IMPLEMENTED !!!"
	if ($env:SPRIG_USE_CAFEX_SETBRIDGE -eq 1) {
	} else {
	}
	exit 1
}
