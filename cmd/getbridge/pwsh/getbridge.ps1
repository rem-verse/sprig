if (-not $env:SPRIG_RUNNING_FROM_SOURCE) {
	$env:SPRIG_RUNNING_FROM_SOURCE=0
}
if ($env:SPRIG_RUNNING_FROM_SOURCE -eq 1) {
	# We use this over as it's not until powershell 3.0 we get proper handling
	# of `$PSScriptRoot`
	& "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\getbridgetype\pwsh\getbridgetype.ps1"
} else {
	& "$(Split-Path $MyInvocation.MyCommand.Path)\getbridgetype"
}

if ($env:BRIDGE_TYPE -eq "Mion") {
	if (-not $args[0]) {
		# This block was not present in the original `getbridge` script, and
		# instead has been ported from `cafe.bat`, to remove a dependency on
		# `cafe.bat`, `cafex_env.bat`, or `mochiato`.
		#
		# The original block was:
		#
		# ```batch
		# if not "%BRIDGE_CURRENT_NAME%"=="" goto :skip_mion
		# if not exist "%MION_BRIDGE_TOOLS%\getbridgeconfig.exe" goto skip_mion
		# :mion_name
		# for /f "usebackq tokens=5" %%i in (`"%MION_BRIDGE_TOOLS%\getbridgeconfig.exe" -default`) do (
		#     set BRIDGE_CURRENT_NAME=%%i
		#     goto mion_ip_address
		# )
		# :mion_ip_address
		# for /f "usebackq skip=1 tokens=5" %%i in (`"%MION_BRIDGE_TOOLS%\getbridgeconfig.exe" -default`) do (
		#     set BRIDGE_CURRENT_IP_ADDRESS=%%i
		# )
		# :skip_mion
		# ```
		if (-not "$env:BRIDGE_CURRENT_NAME") {
			$default_bridges=$null
			if (("$env:SPRIG_RUNNING_FROM_SOURCE" -eq 1) -And (Test-Path -Path "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\..\target\debug\getbridgeconfig.exe")) {
				$default_bridges=& "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\..\target\debug\getbridgeconfig.exe" -default
			} elseif (("$env:SPRIG_RUNNING_FROM_SOURCE" -eq 1) -And (Test-Path -Path "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\..\target\release\getbridgeconfig.exe")) {
				$default_bridges=& "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\..\target\release\getbridgeconfig.exe" -default
			} else {
				$default_bridges=& getbridgeconfig.exe -default
			}
			$default_bridges_lines=$default_bridges -split "`n"
			$bridge_name_line_split=$default_bridges_lines[0] -split ": "
			$bridge_ip_line_split=$default_bridges_lines[1] -split ": "
			$env:BRIDGE_CURRENT_NAME=$bridge_name_line_split[1]
			$env:BRIDGE_CURRENT_IP_ADDRESS=$bridge_ip_line_split[1]
		}

		Write-Output "Current Bridge Name        = $env:BRIDGE_CURRENT_NAME"
		Write-Output "Current Bridge IP Address  = $env:BRIDGE_CURRENT_IP_ADDRESS"
	} else {
		if (("$env:SPRIG_RUNNING_FROM_SOURCE" -eq 1) -And (Test-Path -Path "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\..\target\debug\getbridgeconfig.exe")) {
			& "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\..\target\debug\getbridgeconfig.exe" $args
		} elseif (("$env:SPRIG_RUNNING_FROM_SOURCE" -eq 1) -And (Test-Path -Path "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\..\target\release\getbridgeconfig.exe")) {
			& "$(Split-Path $MyInvocation.MyCommand.Path)\..\..\..\target\release\getbridgeconfig.exe" $args
		} else {
			& getbridgeconfig.exe $args
		}
	}
} else {
	Write-Output "Bridge type invalid: This bridge tool only works with Catdev v3 or ev_x4 and newer devkits"
}
