# --- POWER_SETTINGS_DAEMON_SETUP ---
# This script disables sleep/hibernation to allow the bot to run 24/7.
 
Write-Output "Configuring Windows Power Settings..."
 
# Lid Close Action = Do Nothing (0)
powercfg /setacvalueindex SCHEME_CURRENT SUB_BUTTONS LIDACTION 0
powercfg /setdcvalueindex SCHEME_CURRENT SUB_BUTTONS LIDACTION 0
 
# Standby Timeout = Never (0) when plugged in
powercfg /setacvalueindex SCHEME_CURRENT SUB_SLEEP STANDBYIDLE 0
 
# Hibernate Timeout = Never (0) when plugged in
powercfg /setacvalueindex SCHEME_CURRENT SUB_SLEEP HIBERNATEIDLE 0
 
# Hybrid Sleep = Disabled (0)
powercfg /setacvalueindex SCHEME_CURRENT SUB_SLEEP HYBRIDSLEEP 0
 
# Apply changes
powercfg /setactive SCHEME_CURRENT
 
Write-Output "SUCCESS: Laptop lid close action is now 'Do Nothing'."
Write-Output "SUCCESS: Sleep/Hibernation disabled for AC power."
Write-Output "Bot is ready for 24/7 background operation."
