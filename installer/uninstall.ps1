$ErrorActionPreference='Stop'
$installDir=[System.IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs\Lingdao'))
if([System.IO.Path]::GetFullPath($PSScriptRoot) -ne $installDir){throw '卸载脚本只能在灵岛安装目录中运行。'}
$exePath=Join-Path $installDir 'lingdao.exe'
foreach($p in @('lingdao','daozai')){
    Get-Process $p -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq (Join-Path $installDir ($p+'.exe'))} | ForEach-Object {
        $null=$_.CloseMainWindow();if(-not $_.WaitForExit(3000)){Stop-Process -Id $_.Id}
    }
}
$shell=New-Object -ComObject WScript.Shell
$links=@((Join-Path $shell.SpecialFolders.Item('Desktop') '灵岛.lnk'),(Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\灵岛.lnk'),(Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\灵岛设置.lnk'))
foreach($path in $links){if((Test-Path -LiteralPath $path) -and $shell.CreateShortcut($path).TargetPath -eq $exePath){Remove-Item -LiteralPath $path}}
$runKey='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
Remove-ItemProperty -Path $runKey -Name 'Lingdao' -ErrorAction SilentlyContinue
Remove-ItemProperty -Path $runKey -Name 'Daozai' -ErrorAction SilentlyContinue
Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Lingdao' -ErrorAction SilentlyContinue
Remove-Item -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Daozai' -ErrorAction SilentlyContinue
foreach($name in @('lingdao.exe','lingdao-previous.exe','daozai.exe','daozai-previous.exe','daozai-v0.2-backup.exe','THIRD_PARTY_NOTICES.txt','LICENSE','uninstall.ps1')){
    $path=Join-Path $installDir $name;if(Test-Path -LiteralPath $path){Remove-Item -LiteralPath $path}
}
if(-not(Get-ChildItem -LiteralPath $installDir -Force)){Remove-Item -LiteralPath $installDir}
Write-Output '灵岛已卸载。待办、饮水记录和设置仍保存在本机用户数据目录中。'
