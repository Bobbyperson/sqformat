untyped

global function GamemodeInfection_Init
global function TestEvac
global function NoInfect
global function setTime
global function RateSpawnpoints_Infection
const int EVAC_SHIP_SHIELDS = 2500

const array<string> EVAC_EMBARK_ANIMS_3P = [
	"pt_e3_rescue_side_embark_A",
	"pt_e3_rescue_side_embark_B",
	"pt_e3_rescue_side_embark_C",
	"pt_e3_rescue_side_embark_D",
	"pt_e3_rescue_side_embark_E",
	"pt_e3_rescue_side_embark_F",
	"pt_e3_rescue_side_embark_G",
	"pt_e3_rescue_side_embark_H"
]

const array<string> EVAC_IDLE_ANIMS_1P = [
	"ptpov_e3_rescue_side_embark_A_idle",
	"ptpov_e3_rescue_side_embark_B_idle",
	"ptpov_e3_rescue_side_embark_C_idle",
	"ptpov_e3_rescue_side_embark_D_idle",
	"ptpov_e3_rescue_side_embark_E_idle",
	"ptpov_e3_rescue_side_embark_F_idle",
	"ptpov_e3_rescue_side_embark_G_idle",
	"ptpov_e3_rescue_side_embark_H_idle"
]

const array<string> EVAC_IDLE_ANIMS_3P = [
	"pt_e3_rescue_side_idle_A",
	"pt_e3_rescue_side_idle_B",
	"pt_e3_rescue_side_idle_C",
	"pt_e3_rescue_side_idle_D",
	"pt_e3_rescue_side_idle_E",
	"pt_e3_rescue_side_idle_F",
	"pt_e3_rescue_side_idle_G",
	"pt_e3_rescue_side_idle_H"
]

const array<string> SURVIVOR_HINTS = [
	"Movement! Movement! Movement! Staying in one place is an easy way to get swarmed.",
	"Stick together! Your fellow survivors are key to your survival.",
	"Keep your distance! Infected can punch from deceptively far away.",
	"Beware the variants! Some infected have special abilities.",
	"Explosive weapons can be a great tool to quickly shutdown a horde.",
	"Stick to the shadows! Sometimes it's better to avoid a fight when you can.",
	"Infected health decreases as their numbers increase.",
	"Look up! Sometimes infected find creative paths.",
	"Stay alert! Sometimes infected can get in your face before you see or hear them.",
	"Listen! Their growls are their biggest giveaways."
]

const array<string> INFECTED_HINTS = [
	"Your MGL is just a tool. You will have the best chances with melee.",
	"Mix up your movement! Going in a straight line is easy to track!",
	"Attack together! You're weak alone.",
	"Utilize your grapple! Use flings and creative movement to make yourself harder to track.",
	"Reel them in! Your grapple can stick to survivors.",
	"Don't waste abilities! Save them for when you're chasing survivors.",
	"Watch your radar! Survivors can reveal themselves just by shooting.",
	"Find creative paths! Be unpredictable.",
	"If you execute a survivor, you get their gun."
]


struct {
	bool hasHadFirstInfection = false
	bool hasHadLastInfection = false
	bool hasResetTimer = false
	bool evacCame = false
	bool evacDead = false
	array<entity> playersToNotifyOfInfection
	
	// evac stuff
	array<entity> evacNodes
	table<entity, int> killstreak
	table<entity, int> deathstreak
	table<entity, bool> hardmode
	table<entity, bool> moved
	entity spaceNode
	entity currentEvacNode
	entity evacDropship
	entity evacIcon

	// panel
	array<Point> panelSpawns

	entity queen

	array<string> lowburnreward = [ "burnmeter_ticks", 
									"burnmeter_radar_jammer", 
									"burnmeter_at_turret_weapon",
									"burnmeter_arc_trap",
									"burnmeter_phase_rewind"]

	array<string> medburnreward = [ "burnmeter_ap_turret_weapon",
									"burnmeter_amped_weapons",
									"burnmeter_maphack",
									"burnmeter_ticks", 
									"burnmeter_radar_jammer", 
									"burnmeter_at_turret_weapon",
									"burnmeter_arc_trap",
									"burnmeter_phase_rewind"]

	array<string> highburnreward = ["burnmeter_ap_turret_weapon",
									"burnmeter_amped_weapons",
									"burnmeter_maphack",
									"burnmeter_ticks", 
									"burnmeter_radar_jammer", 
									"burnmeter_at_turret_weapon",
									// "burnmeter_smart_pistol",
									"burnmeter_phase_rewind",
									"burnmeter_arc_trap"]
} file


struct EvacShipSetting
{
	asset shipModel
	string flyinSound
	string hoverSound
	string flyoutSound
}

void function GamemodeInfection_Init()
{
	PrecacheParticleSystem( FX_EVAC_MARKER )
	
	SetLoadoutGracePeriodEnabled( true ) // this is disabled later
	SetWeaponDropsEnabled( false )
	SetShouldUseRoundWinningKillReplay( true )
	SetGamemodeAllowsTeamSwitch( false )
	Riff_ForceTitanAvailability( eTitanAvailability.Never )
	Riff_ForceBoostAvailability( eBoostAvailability.Disabled )
	ClassicMP_ForceDisableEpilogue( true )
	ClassicMP_SetCustomIntro( InfectionNoIntroSetup, Infection_NoIntro_GetLength() )
	// ClassicMP_SetCustomIntro( ClassicMP_DefaultNoIntro_Setup, ClassicMP_DefaultNoIntro_GetLength() )

	SetShouldPlayerStartBleedoutFunc( InfectionShouldPlayerStartBleedout )
	AddCallback_OnClientConnecting( CheckShouldRestartMap )
	AddCallback_OnClientConnected( InfectionInitPlayer )
	AddCallback_OnPlayerKilled( InfectionOnPlayerKilled )
	AddCallback_OnPlayerRespawned( RespawnInfected )
	AddCallback_GameStateEnter( eGameState.Prematch, ResetPanels )
	AddCallback_GameStateEnter( eGameState.Playing, SelectFirstInfected )
	AddCallback_OnReceivedSayTextMessage( HardmodeChat )
	SetTimeoutWinnerDecisionFunc( TimeoutCheckSurvivors ) 

	AddCallback_GameStateEnter( eGameState.WinnerDetermined, OnWinnerDetermined )

	RegisterSignal( "ResetEvac" )
	RegisterSignal( "BoostRefunded" ) // for arc trap to work

	string mapname = GetMapName()
	if (mapname != "mp_wargames"){
		SetSpawnpointGamemodeOverride( FFA )
	}
	switch(mapname)
	{
		case "mp_angel_city":
			file.evacNodes = []
			file.evacNodes.append( CreateScriptRef( < 2527.889893, -2865.360107, 753.002991 >, < 0, -80.54, 0 > ) )
			file.evacNodes.append( CreateScriptRef( < 1253.530029, -554.075012, 811.125 >, < 0, 180, 0 > ) )
			file.evacNodes.append( CreateScriptRef( < 2446.989990, 809.364014, 576.0 >, < 0, 90.253, 0 > ) )
			file.evacNodes.append( CreateScriptRef( < -2027.430054, 960.395020, 609.007996 >, < 0, 179.604, 0 > ) )
			file.spaceNode = CreateScriptRef( < -1700, -5500, -7600 >, < -3.620642, 270.307129, 0 > )
		break

		case "mp_colony02":
			file.evacNodes.append( CreateScriptRef( < -475.129913, 1480.167847, 527.363953 >, < 0, 219.338501, 0 > ) )
			file.evacNodes.append( CreateScriptRef( < 1009.315186, 3999.888916, 589.914917 >, < 0, -146.680725, 0 > ) )
			file.evacNodes.append( CreateScriptRef( < 2282.868896, -1363.706543, 846.188660 >, < 0, -146.680725, 0 > ) )
			file.evacNodes.append( CreateScriptRef( < 1911.771606, -752.053101, 664.741821 >, < 0, 138.721191, 0 > ) )
			file.evacNodes.append( CreateScriptRef( < 1985.563232, -1205.455078, 677.444763 >, < 0, -239.877441, 0 > ) )
			file.evacNodes.append( CreateScriptRef( < -59.625496, -1858.108887, 811.592407 >, < 0, -252.775146, 0 > ) )
			file.evacNodes.append( CreateScriptRef( < -1035.991211, -671.114380, 824.180908 >, < 0, -24.511070, 0 > ) )
			file.spaceNode = CreateScriptRef( < -1700, -5500, -7600 >, < -3.620642, 270.307129, 0 > )
		break

		case "mp_wargames":
			file.evacNodes.append( CreateScriptRef( < -1254.61, 3538.7, 279.014 >, < 0, 90.6091, 0 >))
			file.spaceNode = CreateScriptRef( < -1700, -5500, -7600 >, < -3.620642, 270.307129, 0 > )
		break
		
		case "mp_lf_stacks":
			file.evacNodes.append( CreateScriptRef( < 294.715, 2164.05, 360.226 >, < 0, -90.66, 0 > ) )
			file.spaceNode = ( CreateScriptRef( < -1583.54, 2065.05, 1447.59 >, < 0, -152.03, 0 > ) )
		break
		
		case "mp_lf_deck":
			file.evacNodes.append( CreateScriptRef( < 261.644, -1836.96, 458.099 >, < 0, 89.4534, 0 > ) )
			file.spaceNode = ( CreateScriptRef( < -3546.1, -3402.22, 2210.24 >, < 0, -11.7048, 0 > ) )
		break
		
		case "mp_lf_meadow":
			file.evacNodes.append( CreateScriptRef( < 128.004, -1598.94, 456.993 >, < 0, 89.8562, 0 > ) )
			file.spaceNode = ( CreateScriptRef( < 2501.73, -1671.72, 1436.55 >, < 0, 26.0562, 0 > ) )
		break

		case "mp_lf_traffic":
			file.evacNodes.append( CreateScriptRef( < 354.044, -1735.66, 218.531 >, < 0, 89.1288, 0 > ) )
			file.spaceNode = ( CreateScriptRef( < 4998.45, 110.398, -6136.04 >, < 0, 179.053, 0 > ) )
		break

		case "mp_lf_township":
			file.evacNodes.append( CreateScriptRef( < 1299.63, 130.964, 375.644 >, < 0, -178.711, 0 > ) )
			file.spaceNode = (CreateScriptRef( < 4998.45, 110.398, -6136.04 >, < 0, 179.053, 0 > ) )
		break

		case "mp_lf_uma":
			file.evacNodes.append(CreateScriptRef( < 994.444, 359.271, 218.583 >, < 0, -177.181, 0 > ) )
			file.spaceNode = (CreateScriptRef( < 4998.45, 110.398, -6136.04 >, < 0, 168.053, 0 > ) )
		break

		case "mp_coliseum":
			file.evacNodes.append(CreateScriptRef( < -3.50534, -4.30921, 126.576 >, < 0, 173.968, 0 > ) )
			file.spaceNode = (CreateScriptRef( < 2988.35, 1367.398, 1354.04 >, < 0, 158.053, 0 > ) )
			InfectionAddPanelSpawns([ <4.77621, 27.3504, 6.49609>, <0, 0, 0> ])
		break

		case "mp_coliseum_column":
			file.evacNodes.append(CreateScriptRef( < -3.50534, -4.30921, 126.576 >, < 0, 173.968, 0 > ) )
			file.spaceNode = (CreateScriptRef( < 2988.35, 1367.398, 1354.04 >, < 0, 158.053, 0 > ) )
		break

		case "mp_forwardbase_kodai":
			InfectionAddPanelSpawns([
				< 2472.952392578125, 3212.56591796875, 992.237548828125 >, <0.0, 90.0, 0.0>,
				< 1407.9998779296876, 2856.232421875, 1096.2265625 >, <0.0, 90.0, 0.0>,
				< -402.2103576660156, 2641.76708984375, 960.03125 >, <0.0, 0.0, 0.0>,
				< -319.26348876953127, 3216.9287109375, 1097.03125 >, <0.0, 90.0, 0.0>,
				< -40.30308532714844, 865.5199584960938, 1096.03125 >, <0.0, 180.0, 0.0>,
				< 56.031185150146487, 866.6178588867188, 1096.03125 >, <0.0, 0.0, 0.0>,
				< 1131.0697021484376, 641.03125, 959.978271484375 >, <0.0, 90.0, 0.0>,
				< -768.03125, 545.4030151367188, 960.03125 >, <0.0, 180.0, 0.0>,
				< -1319.96875, 878.9300537109375, 1096.03125 >, <0.0, 0.0, 0.0>,
				< 1181.820068359375, 195.2162322998047, 1096.03125 >, <0.0, 0.0, 0.0>,
				< 734.2090454101563, -2456.636474609375, 953.5282592773438 >, <0.0, -90.0, 0.0>,
				< -679.96875, -1984.6761474609376, 952.03125 >, <0.0, 0.0, 0.0>,
				< -3006.97119140625, 2064.37060546875, 4741.68408203125 >, <0.0, -90.0, 0.0>,
				< -1049.275634765625, 2753.13232421875, 966.0291748046875 >, <0.0, 180.0, 0.0>,
				< -1072.95703125, -103.04417419433594, 961.4425048828125 >, <0.0, -90.0, 0.0>,
				< 2761.6484375, -495.3310852050781, 948.682373046875 >, <0.0, -90.0, 0.0>,
				< 623.96875, 814.5533447265625, 960.03125 >, <0.0, 180.0, 0.0>,
				< 1459.1153564453126, 953.568115234375, 960.03125 >, <0.0, 180.0, 0.0>
			])
		break
		
		default:
		break
	}

	PrecacheModel($"models/creatures/prowler/r2_prowler.mdl")

	AddCallback_EntitiesDidLoad( SpawnPanelsForLevel )
	RegisterWeaponDamageSource( "funnyboomer", "Boomer Explosion" )
	AddCallback_OnClientDisconnected( InfectionOnPlayerDisconnected )
	thread ShowHintsToSurvivors()
}

void function ShowHintsToSurvivors() {
	while ( true ) {
		wait 90
		array<entity> survivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR )
		if ( survivors.len() > 0 ) {
			foreach ( entity player in survivors ) {
				if ( IsValid( player ) ) { 
					NSSendInfoMessageToPlayer( player, SURVIVOR_HINTS.getrandom() )
				}
			}
		}
	}
}

void function CheckShouldRestartMap( entity player )
{
	if ( GetGameState() <= eGameState.Prematch && Time() > 60 * 30 )
		ServerCommand( "changelevel " + GetMapName() )
}

void function InfectionInitPlayer( entity player )
{
	if ( GetGameState() >= eGameState.Playing || file.hasHadFirstInfection )
		InfectPlayer( player, player )
	file.killstreak[player] <- 0
	file.deathstreak[player] <- 0
	file.hardmode[player] <- false
}

void function InfectionOnPlayerDisconnected( entity player )
{
	thread InfectionOnPlayerDisconnectedDelayed(player)
}

// gotta start documenting code even more since this file is getting bigger and bigger each time
void function InfectionOnPlayerDisconnectedDelayed( entity player ) // 7 august 2023
{
	if ( !file.hasHadFirstInfection ) {
		return
	}
	WaitFrame()
	int infectedCount = GetPlayerArrayOfTeam( INFECTION_TEAM_INFECTED ).len()
	if ( infectedCount == 0 )
	{
		// find a random poor soul lmao
		array<entity> survivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR )
		if ( survivors.len() > 0 )
		{
			int selected = RandomInt( survivors.len() )
			entity infected = survivors[ selected ]

			// give a 5 seconds heads up to the selected player and everyone else
			foreach ( entity player in GetPlayerArray() )
				if ( player != infected )
					NSSendAnnouncementMessageToPlayer( player, "NEW HOST FOUND", "A random survivor will be infected in 5 seconds!", <0,1,0>, 1, 0 )
				else
					NSSendAnnouncementMessageToPlayer( player, "YOU HAVE BECOME INFECTED", "You will become infected in 5 seconds!", <0,1,0>, 1, 1 )

			HolsterViewModelAndDisableWeapons( infected )
			wait 5.0
			if ( !IsValid(infected) )
				return
			InfectPlayer( infected, infected )
			RespawnInfected( infected )
			DeployViewModelAndEnableWeapons( infected )
		}
	}
}

void function InfectionInitSpawnPlayer( entity player )
{
	if ( GetGameState() != eGameState.Prematch )
		return

	if ( IsPrivateMatchSpectator( player ) ) // private match spectators use custom spawn logic
	{
		RespawnPrivateMatchSpectator( player )
		return
	}

	if ( IsAlive( player ) )
		player.Die()

	SetTeam( player, INFECTION_TEAM_SURVIVOR )
	player.RespawnPlayer( FindSpawnPoint( player, false, false ) )

	player.FreezeControlsOnServer()
	AddCinematicFlag( player, CE_FLAG_CLASSIC_MP_SPAWNING )
	ScreenFadeFromBlack( player, 0.5, 0.5 )
}

float function Infection_NoIntro_GetLength()
{
	return 10.0
}

void function InfectionNoIntroStart()
{
	ClassicMP_OnIntroStarted()

	foreach ( entity player in GetPlayerArray() )
		InfectionInitSpawnPlayer( player )
		
	wait 10.0
		
	foreach ( entity player in GetPlayerArray() )
	{
		if ( !IsPrivateMatchSpectator( player ) )
		{
			player.UnfreezeControlsOnServer()
			RemoveCinematicFlag( player, CE_FLAG_CLASSIC_MP_SPAWNING )
		}
			
		TryGameModeAnnouncement( player )
	}
	
	ClassicMP_OnIntroFinished()
}

void function InfectionNoIntroSetup()
{
	AddCallback_OnClientConnected( InfectionInitSpawnPlayer )
	AddCallback_GameStateEnter( eGameState.Prematch, InfectionNoIntroStart )
}

// ===============================================================================
//
//
// HARDMODE , 18 October 2023 , edited 30 August 2024
//
//
// ===============================================================================

ClServer_MessageStruct function HardmodeChat(ClServer_MessageStruct message)
{
	string msg = message.message.tolower()

	if (msg.len() == 0 )
		return message

	if (format("%c", msg[0]) == "!") {
		printl("Chat Command Found")
		// command
		msg = msg.slice(1) // remove !

		if (msg == "hardmode")
		{
			message.shouldBlock = true
			entity player = message.player
			if ( player.GetTeam() == INFECTION_TEAM_SURVIVOR && file.hardmode[player] == false && GameTime_TimeLeftSeconds() >= ( 300.0 ) ) {
				GetFuckedLoser( player )
				file.hardmode[player] = true
			} else {
				NSSendInfoMessageToPlayer(player, "NO!!!!!!!")
			}
		} else if (msg == "bark") {
			entity player = message.player
			if ( IsValid( player ) || IsAlive( player ) )
				return message
			if ( player.GetTeam() == INFECTION_TEAM_INFECTED ) {
				EmitSoundOnEntity( player, "prowler_vocal_bark" )
			}
		}
	}
	return message
}

void function GetFuckedLoser( entity player )
{
	// lower health to 20
	player.SetMaxHealth(25)
	player.SetHealth(25)

	// perma sonared
	/* Highlight_SetEnemyHighlight( player, "enemy_boss_bounty" ) // red sonar just to make it extra spicy
	StatusEffect_AddEndless( player, eStatusEffect.sonar_detected, 1.0 ) */

	thread HighlightHardmode( player )

	foreach ( entity weapon in player.GetMainWeapons() )
		player.TakeWeaponNow( weapon.GetWeaponClassName() )

	foreach ( entity weapon in player.GetOffhandWeapons() )
		player.TakeWeaponNow( weapon.GetWeaponClassName() )

	player.GiveWeapon( "mp_weapon_semipistol", ["silencer"] )
	player.GiveOffhandWeapon( "melee_pilot_emptyhanded", OFFHAND_MELEE )

	// xd
	Chat_Impersonate( player, "hey infected im the guy with the red glow and i have very low hp because i think this gamemode is too easy", false)
}

void function HighlightHardmode( entity player )
{
	player.EndSignal( "OnDeath" )
	player.EndSignal( "OnDestroy" )

	var lastHighlightTime = Time()
	var highlightDuration = 5.0
	var cooldownDuration = 5.0
	array<int> ids = []

	while ( IsAlive(player) )
	{
		WaitFrame()
		vector playerVelV = player.GetVelocity()
		float playerVel = sqrt( playerVelV.x * playerVelV.x + playerVelV.y * playerVelV.y ) // not counting vertical velocity
		float playerVelNormal = playerVel * (0.091392) // metric kph

		// 0 - 45 kph is highlighted
		if ( playerVelNormal < 45 )
		{
			Highlight_SetEnemyHighlight( player, "sp_objective_entity" )
			player.Highlight_SetParam( 2, 0, HIGHLIGHT_COLOR_ENEMY )
			ids.append( StatusEffect_AddEndless( player, eStatusEffect.sonar_detected, 1.0 ) )
			lastHighlightTime = Time()
		}
		// 46 - 65 kph is highlighted for 5 seconds every 10 seconds, 
		// once you reach this speed from below 45 kph, you will remain highlighted for 5 seconds before clearing the highlight, then reapplying it after another 5 seconds
		else if ( playerVelNormal >= 45 && playerVelNormal < 66 )
		{
			if ( Time() - lastHighlightTime > cooldownDuration + highlightDuration )
			{
				Highlight_SetEnemyHighlight( player, "sp_objective_entity" )
				player.Highlight_SetParam( 2, 0, HIGHLIGHT_COLOR_ENEMY )
				ids.append( StatusEffect_AddEndless( player, eStatusEffect.sonar_detected, 1.0 ) )
				lastHighlightTime = Time()
			}
			else if ( Time() - lastHighlightTime > highlightDuration )
			{
				Highlight_ClearEnemyHighlight( player )
				foreach ( id in ids )
					StatusEffect_Stop( player, id )
			}
		}
		// 66 - 79 kph is highlighted for 5 seconds every 15 seconds
		else if ( playerVelNormal >= 65 && playerVelNormal < 80 )
		{
			if ( Time() - lastHighlightTime > cooldownDuration + highlightDuration + 5.0 )
			{
				Highlight_SetEnemyHighlight( player, "sp_objective_entity" )
				player.Highlight_SetParam( 2, 0, HIGHLIGHT_COLOR_ENEMY )
				ids.append( StatusEffect_AddEndless( player, eStatusEffect.sonar_detected, 1.0 ) )
				lastHighlightTime = Time()
			}
			else if ( Time() - lastHighlightTime > highlightDuration)
			{
				Highlight_ClearEnemyHighlight( player )
				foreach ( id in ids )
					StatusEffect_Stop( player, id )
			}
		}
		// 80+ kph is not highlighted
		else
		{
			Highlight_ClearEnemyHighlight( player )
			foreach ( id in ids )
				StatusEffect_Stop( player, id )
		}
	}
}

// ===============================================================================
//
//
// PLAYING CODE
//
//
// ===============================================================================

void function SelectFirstInfected() // ?
{
	thread SelectFirstInfectedDelayed()
}

void function SelectFirstInfectedDelayed()
{
	srand(GetUnixTimestamp()) // don't use the same rng seed each time lol
	wait 5.0
	Chat_ServerBroadcast("Join us on discord at \x1b[36mdiscord.awesome.tf", true)
	WaitFrame()
	Chat_ServerBroadcast("Please do not go out of bounds.", true)
	WaitFrame()
	Chat_ServerBroadcast("Please note, this server is temporarily running the V2 branch of Infection for playtesting. Expect crashes.", true)
	wait 5.0 + RandomFloat( 5.0 )
	SetLoadoutGracePeriodEnabled( false )
	WaitFrame()
	ServerCommand("melee_lunge_scale_by_speed 0")
	ServerCommand("sv_alltalk 1");
	ServerCommand("slide_step_velocity_reduction 10");
	ServerCommand("sv_gravity 750");

	int playerCount = GetPlayerArray().len()
	if (playerCount <= 1)
		return

	if ( file.hasHadFirstInfection ){ // in case a player joins and gets infected before this happens
		array<entity> players = GetPlayerArray()
		array<entity> survivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR )
		if (players.len() < 8) {
			Chat_ServerBroadcast("Due to a low playercount all survivors are sonared to keep the pace up. Please try not to sweat and make sure everyone has fun.", true)
			foreach ( entity player in survivors ){
				Highlight_SetEnemyHighlight( player, "enemy_sonar" )
				StatusEffect_AddEndless( player, eStatusEffect.sonar_detected, 1.0 ) // sonar is better here so the player themselves see the SONAR DETECTED warning.
			}
		}
		return
	}
	array<entity> players = GetPlayerArray()

	if (!players.len()) {
		SetWinner( INFECTION_TEAM_SURVIVOR )
		return
	}

	entity infected = players.getrandom()

	if (file.hardmode[infected] == true) {
		infected = players.getrandom()
	}

	InfectPlayer( infected, infected )
	RespawnInfected( infected )

	array<entity> survivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR )
	
	if (GetPlayerArray().len() < 8) {
		Chat_ServerBroadcast("Due to a low playercount all survivors are sonared to keep the pace up.", true)
		foreach ( entity player in survivors ){
			Highlight_SetEnemyHighlight( player, "enemy_sonar" )
			StatusEffect_AddEndless( player, eStatusEffect.sonar_detected, 1.0 ) // sonar is better here so the player themselves see the SONAR DETECTED warning.
		}
	}
}

void function InfectionOnPlayerKilled( entity victim, entity attacker, var damageInfo )
{
	ShouldPanelsBeUsable()
	if ( victim.GetTeam() == INFECTION_TEAM_SURVIVOR ) // this needs to happen first because of death by OOB or other reasons
		InfectPlayer( victim, attacker )

	if ( victim == file.queen ) {
		file.queen = null
	}

	if ( !victim.IsPlayer() || !attacker.IsPlayer() || GetGameState() != eGameState.Playing || !IsValid( attacker ) || !IsValid( victim ) )
		return

	file.killstreak[victim] <- 0

	if ( victim != attacker ) 
	{
		attacker.SetPlayerGameStat( PGS_ASSAULT_SCORE, attacker.GetPlayerGameStat( PGS_ASSAULT_SCORE ) + 1 )
		SetRoundWinningKillReplayAttacker(attacker)


		if ( attacker.GetTeam() == INFECTION_TEAM_SURVIVOR )
		{
			EmitSoundAtPosition( TEAM_UNASSIGNED, victim.GetOrigin(), "prowler_vocal_death" )
			// killstreaks
			file.killstreak[attacker] += 1
			file.deathstreak[victim] += 1
			int kills = file.killstreak[attacker]
			

			// // if the killstreak is a multiple of 5 (5, 10, 15, etc), give a random boost
			// if ( file.killstreak[attacker] > 4 && ( file.killstreak[attacker] % 5 == 0 ) && MapSettings_SupportsTitans( GetMapName() ) ) // prevents killstreaks on lf and coliseum
			// {
			// 	string burncard
			// 	switch ( file.killstreak[attacker] )
			// 	{
			// 		// get a random boost from the array
			// 		case 5:
			// 			burncard = file.lowburnreward.getrandom()
			// 			break

			// 		case 10:
			// 			burncard = file.medburnreward.getrandom()
			// 			break

			// 		default: // after 10 they are all high rewards
			// 			burncard = file.highburnreward.getrandom()
			// 			break
			// 	}

			// 	if ( file.killstreak[attacker] == 50 )
			// 	{
			// 		if ( !attacker.IsTitan() && SpawnPoints_GetTitan().len() > 0 )
			// 		{
			// 			thread CreateTitanForPlayerAndHotdrop( attacker, GetTitanReplacementPoint( attacker, false ) )
			// 			foreach ( entity player in GetPlayerArray() )
			// 				if ( player != attacker )
			// 					NSSendLargeMessageToPlayer( player, "TITANFALL INCOMING", attacker.GetPlayerName() + " has obtained a titan!", 7, "rui/callsigns/callsign_68_col")
			// 		}
			// 		else 
			// 			BurnMeter_GiveRewardDirect( attacker, burncard )
			// 	}
			// 	else
			// 		BurnMeter_GiveRewardDirect( attacker, burncard )
			// 	NSSendPopUpMessageToPlayer( attacker, file.killstreak[attacker].tostring() + " killstreak!" )
			// }
		}
		else // if attacker is infected
		{
			// attacker is infected and got a kill
			file.deathstreak[attacker] <- 0

			if ( DamageInfo_GetDamageSourceIdentifier( damageInfo ) == eDamageSourceId.human_execution && GetGameState() == eGameState.Playing && attacker.GetTeam() == INFECTION_TEAM_INFECTED ) // 4 March 2024, if died to execution drop gun
			{
				if ( !IsValid( victim ) )
					return
				array<entity> weapons = GetPrimaryWeapons( victim )
				if ( weapons.len() == 0 )
					return
				entity weapon = weapons[0]
				attacker.GiveWeapon( weapon.GetWeaponClassName() )
			}
		}
	} else {
		array<entity> players = GetPlayerArray()
		SetRoundWinningKillReplayAttacker(players.getrandom())
	}

	// remove boosts from dead players
	//thread PlayerInventory_TakeAllInventoryItems( victim )
	// this appears to not be needed as switching teams removes boosts.
	if ( IsValid( victim.GetPetTitan() ) && victim.GetTeam() == INFECTION_TEAM_SURVIVOR ) // kill any titans if exists
		victim.GetPetTitan().Destroy()
}

void function InfectPlayer( entity player, entity attacker ) {
	ShouldPanelsBeUsable()
	
	SetTeam( player, INFECTION_TEAM_INFECTED )
	player.SetPlayerGameStat( PGS_ASSAULT_SCORE, 0 ) // reset kills
	file.playersToNotifyOfInfection.append( player )
	array<entity> infected = GetPlayerArrayOfTeam( INFECTION_TEAM_INFECTED )
	array<entity> survivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR )
	int playerCount = GetPlayerArray().len()
	int infectedCount = GetPlayerArrayOfTeam( INFECTION_TEAM_INFECTED ).len()
	int survivorCount = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR ).len()
	float health = ((1-(infectedCount.tofloat()/playerCount.tofloat())) * 200)
	int intHealth = health.tointeger()
	foreach (entity infectedPlayer in infected){
		if (IsAlive( infectedPlayer )){
			if( GetGameState() == eGameState.Postmatch ){
				return
			}  
			if(infectedPlayer.IsTitan()){
				continue
			}
			if ( infectedPlayer == file.queen ) {
				continue
			}
			if ( IsValid( file.queen ) ){
				intHealth += 25
			}
			if (infectedCount == 1) { // impossible to win without
				infectedPlayer.SetMaxHealth( 200 )
			}
			else {
				if (infectedCount == playerCount){
					infectedPlayer.SetMaxHealth( 100 )
				} else {
					infectedPlayer.SetMaxHealth( intHealth )
				}
			}
		}
	}
	// check how many survivors there are
	if ( survivors.len() == 0 )
	{
		SetRespawnsEnabled( false )
		SetKillcamsEnabled( false )
		if (attacker.IsPlayer()){
			SetRoundWinningKillReplayAttacker(attacker)
		} else {
			array<entity> infected = GetPlayerArrayOfTeam( INFECTION_TEAM_INFECTED )
			SetRoundWinningKillReplayAttacker(infected[0])
		}
		AddTeamScore( INFECTION_TEAM_INFECTED, 100 ) // for kill replay to work lmao
		SetWinner( INFECTION_TEAM_INFECTED )
	}
	else if ( survivors.len() == 1 && !file.hasHadLastInfection && playerCount > 2 )
		SetLastSurvivor( survivors[ 0 ] )

	if ( !file.hasHadFirstInfection )
	{
		SetLoadoutGracePeriodEnabled( false )
		file.hasHadFirstInfection = true

		foreach ( entity otherPlayer in GetPlayerArray() )
			if ( player != otherPlayer )
				Remote_CallFunction_NonReplay( otherPlayer, "ServerCallback_AnnounceFirstInfected", player.GetEncodedEHandle() )

		PlayMusicToAll( eMusicPieceID.GAMEMODE_1 )
		
		thread CountdownToEvac()
		thread InfectionTimedFunctions()
	}
}

void function RespawnInfected( entity player )
{
	if ( player.GetTeam() != INFECTION_TEAM_INFECTED )
		return

	if (file.hardmode[player] == true) {
		file.hardmode[player] = false
	}

	// notify newly infected players of infection
	if ( file.playersToNotifyOfInfection.contains( player ) )
	{
		Remote_CallFunction_NonReplay( player, "ServerCallback_YouAreInfected" )
		file.playersToNotifyOfInfection.remove( file.playersToNotifyOfInfection.find( player ) )
	}

	// set camo to pond scum
	player.SetSkin( 1 )
	player.SetCamo( 110 )

	// if human, remove helmet bodygroup, human models have some weird bloody white thing underneath their helmet that works well for this, imo
	if ( !player.IsMechanical() )
		player.SetBodygroup( player.FindBodyGroup( "head" ), 1 )

	// stats for infected
	//StimPlayer( player, 9999.9 ) // can't do endless since we don't get the visual effect in endless

	// scale health with num of infected
	int playerCount = GetPlayerArray().len()
	int infectedCount = GetPlayerArrayOfTeam( INFECTION_TEAM_INFECTED ).len()
	int survivorCount = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR ).len()
	float health = ((1-(infectedCount.tofloat()/playerCount.tofloat())) * 200)
	int intHealth = health.tointeger()
	if ( IsValid(file.queen) ) {
		intHealth += 25
	}
	if (infectedCount == 1) { // custom logic for the first infected
		if (file.deathstreak[player] > 4) {
			player.SetMaxHealth( 200 + (file.deathstreak[player] * 10) )
			if (file.deathstreak[player] % 5 == 0) {
				thread DisplayIncreasedHealth( player )
			}
		} else {
			player.SetMaxHealth( 200 )
		}
	}
	else
	{
		if (infectedCount == playerCount) {
			player.SetMaxHealth(100)
		} else { 
			player.SetMaxHealth(intHealth)

			if ( file.deathstreak[player] > 4 && infectedCount < 4 )
				player.SetMaxHealth( intHealth + ( file.deathstreak[player] * 10 ) ) // 10 extra health per death after the 4th death, so they spawn with 50, 60, 70 and so on.
		}
	}

	// set loadout
	foreach ( entity weapon in player.GetMainWeapons() )
		player.TakeWeaponNow( weapon.GetWeaponClassName() )

	foreach ( entity weapon in player.GetOffhandWeapons() )
		player.TakeWeaponNow( weapon.GetWeaponClassName() )

	// in the far future when MAD releases we set MGL to a secondary and have the primary be a custom melee weapon

	player.GiveOffhandWeapon( "melee_pilot_emptyhanded", OFFHAND_MELEE )

	int specialRoll = RandomInt(5)
	bool special = false
	if (specialRoll == 0){
		special = true
	}

	bool queenExists = false
	if ( IsValid( file.queen ) )
		queenExists = true

	int queenRoll = RandomInt(50)
	bool queen = false
	if (queenRoll == 0 && !queenExists && infectedCount > 1) {
		queen = true
		special = false
	}

	if (special) {
		int specialType = RandomInt(4)
		// disallow hunter with less than 8 players
		if (playerCount < 8 || survivorCount == 1) {
			while (specialType == 2) {
				specialType = RandomInt(4)
			}
		}
		switch (specialType) {

			// boomer
			case 0:
				thread BecomeBoomer( player )
				thread NSSendAnnouncementMessageToPlayer_Delayed( player, "VIRUS MUTATION", "Boomer Variant!", <1,0,0>, 1, 0 )
				thread HandleHighlight( player, <1,0,0> )
			break

			// shifter
			case 1:
				player.GiveOffhandWeapon("mp_ability_grapple", OFFHAND_SPECIAL)
				player.GiveOffhandWeapon("mp_ability_shifter", OFFHAND_ORDNANCE)
				player.GiveWeapon( "mp_weapon_mgl" )
				thread NSSendAnnouncementMessageToPlayer_Delayed( player, "VIRUS MUTATION", "Shifter Variant!", <0,1,0>, 1, 0 )
				thread HandleHighlight( player, <0,1,0> )
			break

			// hunter
			case 2:
				thread BecomeHunter( player )
				player.GiveWeapon( "mp_weapon_mgl" )
				player.GiveOffhandWeapon("mp_ability_grapple", OFFHAND_SPECIAL)
				player.GiveOffhandWeapon("mp_ability_heal", OFFHAND_ORDNANCE )
				thread NSSendAnnouncementMessageToPlayer_Delayed( player, "VIRUS MUTATION", "Seeker Variant!", <1,0,1>, 1, 0 )
				thread HandleHighlight( player, <1,0,1> )
			break

			// spitter
			case 3:
				player.GiveOffhandWeapon("mp_ability_heal", OFFHAND_SPECIAL)
				player.GiveOffhandWeapon("mp_weapon_thermite_grenade", OFFHAND_ORDNANCE)
				player.GiveWeapon( "mp_weapon_mgl" )
				thread NSSendAnnouncementMessageToPlayer_Delayed( player, "VIRUS MUTATION", "Spitter Variant!", <0,0,1>, 1, 0 )
				thread HandleHighlight( player, <0,0,1> )
			break

			// default
			default:
				printt("Programming error.")
			break
		}
	} else if (queen) {
		StimPlayer( player, 9999.9 )
		player.GiveWeapon( "mp_weapon_mgl" )
		thread NSSendAnnouncementMessageToPlayer_Delayed( player, "YOU ARE THE HIVE QUEEN", "All infected spawn near you!", <1,1,1>, 1, 0 )
		thread HandleHighlight( player, <1,0,0>, true )
		file.queen = player
		player.SetMaxHealth( 250 )
		foreach ( entity survivorPlayer in GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR ) ) {
			thread NSSendAnnouncementMessageToPlayer_Delayed( survivorPlayer, "HIVE QUEEN DETECTED", "Eliminate it ASAP to weaken the infection!", <1,0,0>, 1, 0 )
		}
		foreach ( entity infectedPlayer in GetPlayerArrayOfTeam( INFECTION_TEAM_INFECTED ) ) {
			if (infectedPlayer != player) {
				thread NSSendAnnouncementMessageToPlayer_Delayed( infectedPlayer, "HIVE QUEEN DETECTED", "You feel stronger! Protect the Hive Queen at all costs!", <1,1,1>, 1, 0 )
			}
		}
	} else {
		player.GiveOffhandWeapon("mp_ability_grapple", OFFHAND_SPECIAL )
		player.GiveOffhandWeapon("mp_ability_heal", OFFHAND_ORDNANCE )
		player.GiveWeapon( "mp_weapon_mgl" )
		if (RandomInt(10) == 0) {
			NSSendInfoMessageToPlayer( player, INFECTED_HINTS.getrandom() ) // don't want to do this for special infected because they might have special info text
		}
	}

	// Default spawn algorithms, even when modified to spawn near the queen, will always seem to take into account
	// LOS and nearby survivors. This seems to be the best compromise for now.
	array<entity> spawnPoints = SpawnPoints_GetPilot()
	entity closestSpawnPoint = spawnPoints[0]
	if (queenExists) {
		foreach (entity spawnPoint in spawnPoints) {
			if (DistanceSqr(spawnPoint.GetOrigin(), file.queen.GetOrigin()) < DistanceSqr(closestSpawnPoint.GetOrigin(), file.queen.GetOrigin())) {

				bool valid = true;
				foreach (entity survivor in GetPlayerArrayOfTeam(INFECTION_TEAM_SURVIVOR)) {
					if (DistanceSqr(spawnPoint.GetOrigin(), survivor.GetOrigin()) < 500) {
						valid = false
						break
					}
				}

				if (valid) {
					closestSpawnPoint = spawnPoint
				}
			}
		}
		player.SetOrigin(closestSpawnPoint.GetOrigin())
	}


	thread PlayInfectedSounds( player )
}

// this exists because sometimes the message is sent when a non-NS announcement is sent, which causes it to not appear ever
void function NSSendAnnouncementMessageToPlayer_Delayed( entity player, string title, string description, vector color, int priority, int style ){
	wait 1.0
	NSSendAnnouncementMessageToPlayer( player, title, description, color, priority, style )
}

void function HandleHighlight( entity player, vector color, bool wallhacks = false )
{
	// sp_friendly_pilot is like the only one that doesnt go through walls
	if ( wallhacks ) {
		Highlight_SetFriendlyHighlight( player, "sp_objective_entity" )
		player.Highlight_SetParam( 1, 0, color )
		Highlight_SetEnemyHighlight( player, "sp_objective_entity" )
		player.Highlight_SetParam( 2, 0, color )
	} else {
		Highlight_SetFriendlyHighlight( player, "sp_friendly_pilot" )
		player.Highlight_SetParam( 1, 0, color )
		Highlight_SetEnemyHighlight( player, "sp_friendly_pilot" )
		player.Highlight_SetParam( 2, 0, color )
	}

	while ( IsAlive(player) && IsValid(player) ) {
		WaitFrame()
	}

	if ( !IsValid(player) )
		return

	Highlight_ClearEnemyHighlight( player )
	Highlight_ClearFriendlyHighlight( player )
}

void function DisplayIncreasedHealth( entity player )
{
	wait 1.0
	if ( !IsValid(player) )
		return
	NSSendAnnouncementMessageToPlayer( player, "VIRAL RESILIENCE", "Your health has been increased!", <1,1,1>, 1, 0 )
	wait 1.0
	array<entity> survivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR )

	foreach ( entity survivor in survivors) {
		if ( IsValid(survivor) ) {
			NSSendAnnouncementMessageToPlayer( survivor, "MUTATION DETECTED", "An infected with increased health has been detected!", <1,0,0>, 1, 0 )
		}
	}
}

void function BecomeBoomer( entity player )
{
	player.EndSignal( "OnDeath" )
	// Remove all weapons
	foreach ( entity weapon in player.GetMainWeapons() )
		player.TakeWeaponNow( weapon.GetWeaponClassName() )

	foreach ( entity weapon in player.GetOffhandWeapons() )
		player.TakeWeaponNow( weapon.GetWeaponClassName() )

	if ( RandomInt(10) == 0 ) { // give phase
		player.GiveOffhandWeapon("mp_ability_shifter_super", OFFHAND_SPECIAL ) // toggle phase shift, side effect is that you slowly grow blind while in the phase dimension
		array<entity> survivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR )
		foreach ( entity survivor in survivors) {
			thread NSSendAnnouncementMessageToPlayer_Delayed( survivor, "MUTATION DETECTED", "A phase boomer has spawned!", <1,0,0>, 1, 0 )
		}
	}
	else
		player.GiveOffhandWeapon("mp_ability_grapple", OFFHAND_SPECIAL )

	// required to allow boomer to use offhands, they explode anyways if they try to use melee
	player.GiveOffhandWeapon( "melee_pilot_emptyhanded", OFFHAND_MELEE, [ "allow_as_primary" ])
	player.SetActiveWeaponByName( "melee_pilot_emptyhanded" )

	// health no longer nerfed
	//player.SetMaxHealth( player.GetMaxHealth() )
	//player.SetHealth( player.GetMaxHealth() )

	// Change enemy highlight color to orange in RGB
	Highlight_SetEnemyHighlightWithParam0( player, "enemy_player", < 255, 165, 0 > )

	// Give amped battery trail FX
	int attachID = player.LookupAttachment( "CHESTFOCUS" )
	int friendlyTeam = player.GetTeam()
	entity boomerFX = StartParticleEffectOnEntity_ReturnEntity( player, GetParticleSystemIndex( BATTERY_FX_AMPED) , FX_PATTACH_POINT_FOLLOW, attachID )
	SetTeam( boomerFX, friendlyTeam )
	boomerFX.SetOwner( player )
	boomerFX.kv.VisibilityFlags = (ENTITY_VISIBLE_TO_FRIENDLY | ENTITY_VISIBLE_TO_ENEMY) // visible except owner

	// callback for player input
	AddButtonPressedPlayerInputCallback( player, IN_ATTACK, BoomerAttack )
	AddButtonPressedPlayerInputCallback( player, IN_MELEE, BoomerAttack )
	// AddButtonPressedPlayerInputCallback( player, IN_OFFHAND1, PhaseExplode )

	OnThreadEnd(
	function() : ( player, boomerFX )
		{
			RemoveButtonPressedPlayerInputCallback( player, IN_ATTACK, BoomerAttack )
			RemoveButtonPressedPlayerInputCallback( player, IN_MELEE, BoomerAttack )
			// RemoveButtonPressedPlayerInputCallback( player, IN_OFFHAND1, PhaseExplode )
			if ( IsValid( boomerFX ) )
				boomerFX.Destroy()
		}
	)

	// Inform the player
	NSSendInfoMessageToPlayer(player, "Press %%attack%% to explode!")

	WaitSignal( player, "StartPhaseShift" ) // replacement for WaitForever, would only get triggered if the player has phase shift and activated it.
	RemoveButtonPressedPlayerInputCallback( player, IN_ATTACK, BoomerAttack )
	RemoveButtonPressedPlayerInputCallback( player, IN_MELEE, BoomerAttack )
	thread PhaseExplodeThreaded( player )
}

void function PhaseExplodeThreaded( entity player )
{
	WaitSignal( player, "StopPhaseShift" )
	BoomerAttack( player )
}

void function BoomerAttack( entity player )
{

	// create an explosion at the player's location
	PlayFX( $"P_impact_exp_FRAG_metal", player.GetOrigin(), < -90.0, 0.0, 0.0 > )
	EmitSoundOnEntityExceptToPlayer( player, player, "explo_fraggrenade_impact_3p" )
	EmitSoundAtPositionOnlyToPlayer( TEAM_UNASSIGNED, player.GetOrigin(), player, "explo_fraggrenade_impact_1p" )
	RadiusDamage(
		player.EyePosition(),												// origin
		player,												// owner
		player,		 									// inflictor
		200,							// pilot damage
		2500,									// heavy armor damage aka titans :)
		5,					// inner radius
		320,					// outer radius
		SF_ENVEXPLOSION_NO_NPC_SOUND_EVENT,					// explosion flags
		0, 													// distanceFromAttacker
		0, 													// explosionForce
		DF_EXPLOSION | DF_GIB,										// damage flags
		eDamageSourceId.funnyboomer		// damage source id
	)
}

void function BecomeHunter( entity player )
{
	player.EndSignal( "OnDeath" )
	player.EndSignal( "OnDestroy" )

	// every 10 seconds, emit a sonar pulse
	while (true)
	{
		wait 10.0
		PlayFX( $"exp_sonar_pulse", player.GetOrigin(), < -90.0, 0.0, 0.0 > )
		PulseLocation( player, player.GetTeam(), player.GetOrigin(), false, false )
		EmitSoundAtPositionExceptToPlayer( TEAM_UNASSIGNED, player.GetOrigin(), player, "Titan_Tone_SonarLock_Impact_Pulse_3P" )
		EmitSoundAtPositionOnlyToPlayer( TEAM_UNASSIGNED, player.GetOrigin(), player, "Titan_Tone_SonarLock_Impact_Pulse_1P" )
		EmitSoundOnEntity( player, "prowler_vocal_howl")
	}
}

void function CountdownToEvac()
{
	int numSurvivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR ).len()
	wait 30.0
	while ( GetGameState() == eGameState.Playing && numSurvivors > 1 )
	{
		if ( GameTime_TimeLeftSeconds() <= 60.0 )
		{
			numSurvivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR ).len()

			float EVAC_WAIT_TIME, EVAC_ARRIVAL_TIME, EVAC_INITIAL_WAIT
			EVAC_INITIAL_WAIT = 0.0
			EVAC_ARRIVAL_TIME = 30 // 50%
			EVAC_WAIT_TIME = 15.0

			PlayMusicToAll( eMusicPieceID.LEVEL_LAST_MINUTE )
			thread SurvivorEvac( INFECTION_TEAM_SURVIVOR, EVAC_INITIAL_WAIT, EVAC_ARRIVAL_TIME, EVAC_WAIT_TIME, CanPlayerBoardEvac, EvacEpilogueShouldLeaveEarly, EvacEpilogueCompleted )
			return
		}

		wait 1.0
	}
}

void function InfectionTimedFunctions()
{
	// ensure shitters are still infected if evac dies
	while (true)
	{
		if (file.evacDead) 
		{
			if ( GameTime_TimeLeftSeconds() <= 5.0 )
			{
				array<entity> survivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR )

				foreach ( entity player in survivors)
					InfectPlayer( player, player )
			}
		}
		wait 1.0
	}
}

void function TestEvac() // mostly deprecated
{
	thread SurvivorEvac( INFECTION_TEAM_SURVIVOR, 1.0, 20.0, 20.0, CanPlayerBoardEvac, EvacEpilogueShouldLeaveEarly, EvacEpilogueCompleted )
}

void function NoInfect() // used for events in-game
{
	file.hasHadFirstInfection = true
}

void function PlayInfectedSounds( entity player )
{
	player.EndSignal( "OnDeath" )
	player.EndSignal( "OnDestroy" )

	float nextRandomSound
	while ( true )
	{
		WaitFrame()

		int meleeState = player.PlayerMelee_GetState()
		if ( nextRandomSound < Time() || meleeState != 0 )
		{
			array<string> infectedSounds = [ "prowler_vocal_attack", "prowler_vocal_attackmiss", "prowler_vocal_attackfast", "prowler_vocal_growl_small", "prowler_vocal_growl_large", "prowler_vocal_pissed", "prowler_vocal_bark" ]
			string selectedSound
			selectedSound = infectedSounds.getrandom()

			bool canSeeSurvivor
			foreach ( entity survivor in GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR ) )
				if ( TraceLineSimple( player.GetOrigin(), survivor.GetOrigin(), survivor ) == 1.0 )
					canSeeSurvivor = true

			// _int sounds are less agressive so only play them if we aren't in some sorta fight
			if ( player.GetHealth() == player.GetMaxHealth() || !canSeeSurvivor || meleeState != 0 )
				selectedSound += "_int"

			EmitSoundOnEntity( player, selectedSound )
			EmitAISoundWithOwner( player, SOUND_PLAYER, 0, player.GetOrigin(), 1600, 2 ) 
			// void EmitAISoundWithOwner( entity owner, int soundFlags, int contextFlags, vector pos, float radius, float duration )
			// zero documentation/example code on soundFlags and contextFlags btw.... no idea what they do

			nextRandomSound = Time() + max( 2.5, RandomFloat( 12.0 ) )
			while ( player.PlayerMelee_GetState() != 0 ) // need to ensure this is updated
				WaitFrame()
		}
	}
}

void function setTime( float time )
{
	SetServerVar( "gameEndTime", Time() + time )
}

void function SetLastSurvivor( entity player )
{
	int playerCount = GetPlayerArray().len()
	foreach ( entity otherPlayer in GetPlayerArray() )
		Remote_CallFunction_NonReplay( otherPlayer, "ServerCallback_AnnounceLastSurvivor", player.GetEncodedEHandle() )

	if ((GameTime_TimeLeftSeconds() > 45) && !file.evacCame && (playerCount > 7)) {
		if ( SpawnPoints_GetTitan().len() > 0) {
			thread CreateTitanForPlayerAndHotdrop( player, GetTitanReplacementPoint( player, false ) )
		} else {
			player.SetMaxHealth( 125 )
		}
		SetServerVar( "gameEndTime", Time() + 45.0 )
		PlayMusicToAll( eMusicPieceID.LEVEL_LAST_MINUTE )
	}

	if ( !file.evacCame ) {
		thread HandleHighlight( player, <1,0,0>, true )
		StatusEffect_AddEndless( player, eStatusEffect.sonar_detected, 1.0 ) // sonar is better here so the player themselves see the SONAR DETECTED warning.
	}

	file.hasHadLastInfection = true

	// if ( IsValid( GetEnt("npc_dropship") ) )
	// 	return
	
	//float EVAC_WAIT_TIME, EVAC_ARRIVAL_TIME, EVAC_INITIAL_WAIT

	//EVAC_INITIAL_WAIT = 0.0
	//EVAC_ARRIVAL_TIME = GameTime_TimeLeftSeconds().tofloat() / 2 // 50%
	//EVAC_WAIT_TIME = 10.0

	//thread SurvivorEvac( INFECTION_TEAM_SURVIVOR, EVAC_INITIAL_WAIT, EVAC_ARRIVAL_TIME, EVAC_WAIT_TIME, CanPlayerBoardEvac, EvacEpilogueShouldLeaveEarly, EvacEpilogueCompleted )
}

int function TimeoutCheckSurvivors()
{
	array<entity> survivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR )
	if ( survivors.len() > 0 )
	{
		SetRespawnsEnabled( false )
		SetKillcamsEnabled( false )
		SetRoundWinningKillReplayAttacker(survivors[ 0 ])
		AddTeamScore( INFECTION_TEAM_SURVIVOR, 100 ) // for kill replay to work lmao
		return INFECTION_TEAM_SURVIVOR
	}
	return INFECTION_TEAM_INFECTED
}

bool function InfectionShouldPlayerStartBleedout( entity player, var damageInfo )
{
	return player.GetTeam() != INFECTION_TEAM_INFECTED
}

void function OnWinnerDetermined()
{
	SetRespawnsEnabled( false )
	SetKillcamsEnabled( false ) // apparently does not affect final killcam
	if (RandomInt(10) == 0) {
		Chat_ServerBroadcast("gg", true)
	}
	Chat_ServerBroadcast("Join us on discord at \x1b[36mdiscord.awesome.tf", true)
	Chat_ServerBroadcast("Please let us know if you have any feedback.", true)
}

// ============================================================================================================================================
// 
// ███████ ██    ██  █████   ██████ 
// ██      ██    ██ ██   ██ ██      
// █████   ██    ██ ███████ ██      
// ██       ██  ██  ██   ██ ██      
// ███████   ████   ██   ██  ██████
//
// ============================================================================================================================================

void function EvacSpectatorFunc( entity player )
{
	svGlobal.levelEnt.EndSignal( "GameStateChanged" )
	file.evacDropship.EndSignal( "OnDestroy" )
	
	entity cam = GetEnt( expect string( file.currentEvacNode.kv.target ) )
	if ( !IsValid( cam ) )
		return
	
	player.SetObserverModeStaticPosition( cam.GetOrigin() )
	player.SetObserverModeStaticAngles( cam.GetAngles() )
	player.StartObserverMode( OBS_MODE_STATIC )
	
	file.evacDropship.WaitSignal( "EvacOver" )
}

void function SurvivorEvac( int evacTeam, float initialWait, float arrivalTime, float waitTime, bool functionref( entity, entity ) canBoardCallback, bool functionref( entity ) shouldLeaveEarlyCallback, void functionref( entity ) completionCallback, entity customEvacNode = null )
{
	if (GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR ).len() == 1)
		if (GetPlayerArray().len() != 1)
			return
	file.evacCame = true
	svGlobal.levelEnt.Signal( "ResetEvac" )
	wait 0.1
	svGlobal.levelEnt.EndSignal( "ResetEvac" )

	EvacShipSetting evacShip = GetEvacShipSettingByTeam( evacTeam )
	
	wait initialWait 

	// setup evac nodes if not manually registered
	if ( file.evacNodes.len() == 0 && !IsValid( customEvacNode ) )
	{
		for ( int i = 1; ; i++ )
		{
			entity newNode = GetEnt( "escape_node" + i )
			if ( !IsValid( newNode ) )
				break
			
			file.evacNodes.append( newNode )
		}
	}
	
	// setup space node if not manually registered
	if ( !IsValid( file.spaceNode ) )
		file.spaceNode = GetEnt( "spaceNode" )

	entity evacNode = customEvacNode
	if ( !IsValid( customEvacNode ) )
		evacNode = file.evacNodes.getrandom()
		
	file.currentEvacNode = evacNode

	// setup client evac position
	file.evacIcon = CreateEntity( "info_target" )
	file.evacIcon.SetOrigin( evacNode.GetOrigin() )
	file.evacIcon.kv.spawnFlags = SF_INFOTARGET_ALWAYS_TRANSMIT_TO_CLIENT
	DispatchSpawn( file.evacIcon )
		// if time remaining is 25% of original time limit
		//GameMode_GetTimeLimit( GameRules_GetGameMode() ) * 60 ) / 100 * 25
	file.evacIcon.DisableHibernation()

	int index = GetParticleSystemIndex( FX_EVAC_MARKER )

	entity effectFriendly = StartParticleEffectInWorld_ReturnEntity( index, evacNode.GetOrigin(), < 0,0,0 > )
	SetTeam( effectFriendly, evacTeam )
	effectFriendly.kv.VisibilityFlags = ENTITY_VISIBLE_TO_FRIENDLY

	wait 0.5 // need to wait here, or the target won't appear on clients for some reason
	// eta until arrive
	SetTeamActiveObjective( evacTeam, "EG_DropshipExtract", Time() + arrivalTime, file.evacIcon )
	SetTeamActiveObjective( GetOtherTeam( evacTeam ), "EG_StopExtract", Time() + arrivalTime, file.evacIcon )
	
	// would've liked to use cd_dropship_rescue_side_start length here, but can't since this is done before dropship spawn, can't
	wait arrivalTime - 4.33333

	entity dropship = CreateDropship( evacTeam, evacNode.GetOrigin(), evacNode.GetAngles() )

	thread DropShipTempHide( dropship ) // prevent showing model and health bar on spawn

	// nessie rework
	//dropship.SetModel( $"models/vehicle/crow_dropship/crow_dropship_hero.mdl" ) 
	//dropship.SetValueForModelKey( $"models/vehicle/crow_dropship/crow_dropship_hero.mdl" )
	dropship.SetModel( evacShip.shipModel )
	dropship.SetValueForModelKey( evacShip.shipModel )
	// rework end
	int playerCount = GetPlayerArray().len()
	int evac_ship_health = (playerCount + 1) * 3125
	dropship.SetMaxHealth( evac_ship_health )
	dropship.SetHealth( evac_ship_health )
	dropship.SetShieldHealth( EVAC_SHIP_SHIELDS )
	SetTargetName( dropship, "#NPC_EVAC_DROPSHIP" )
	DispatchSpawn( dropship )
	// reduce nuclear core's damage
	AddEntityCallback_OnDamaged( dropship, EvacDropshipDamaged )
    AddEntityCallback_OnKilled( dropship, EvacDropshipKilled )
	
	dropship.s.evacSlots <- [ null, null, null, null, null, null, null, null ]
	file.evacDropship = dropship
	
	dropship.EndSignal( "OnDestroy" )
	OnThreadEnd( function() : ( evacTeam, completionCallback, dropship ) 
	{
		if ( "evacTrigger" in dropship.s )
			dropship.s.evacTrigger.Destroy()
		
		// this should be for both teams
		if( !IsValid( dropship ) )
		{
			SetTeamActiveObjective( evacTeam, "EG_DropshipExtractDropshipDestroyed" )
			SetTeamActiveObjective( GetOtherTeam( evacTeam ), "EG_DropshipExtractDropshipDestroyed" )
			
			foreach( entity player in GetPlayerArrayOfTeam( evacTeam ) )
				SetPlayerChallengeEvacState( player, 0 )
		}
	
		foreach ( entity player in dropship.s.evacSlots )
		{
			if ( !IsValid( player ) )
				continue
			
			player.ClearInvulnerable()
		}
		
		// this is called whether dropship is destroyed or evac finishes, callback can handle this itself
		thread completionCallback( dropship )
	})
	
	// flyin
	Spectator_SetCustomSpectatorFunc( EvacSpectatorFunc )
	thread PlayAnim( dropship, "cd_dropship_rescue_side_start", evacNode )

	// fly in sound and effect
	EmitSoundOnEntity( dropship, evacShip.flyinSound )
	thread WarpInEffectEvacShip( dropship )
	
	// calculate time until idle start
	float sequenceDuration = dropship.GetSequenceDuration( "cd_dropship_rescue_side_start" )
	float cycleFrac = dropship.GetScriptedAnimEventCycleFrac( "cd_dropship_rescue_side_start", "ReadyToLoad" )
	wait sequenceDuration * cycleFrac
	
	thread PlayAnim( dropship, "cd_dropship_rescue_side_idle", evacNode )

	// hover sound
	EmitSoundOnEntity( dropship, evacShip.hoverSound )
	
	// eta until leave
	SetTeamActiveObjective( evacTeam, "EG_DropshipExtract2", Time() + waitTime, file.evacIcon )
	SetTeamActiveObjective( GetOtherTeam( evacTeam ), "EG_StopExtract2", Time() + waitTime, file.evacIcon )	

	// dialogue
	PlayFactionDialogueToTeam( "mp_evacGo", evacTeam )
	PlayFactionDialogueToTeam( "mp_evacStop", GetOtherTeam( evacTeam ) )

	// stop evac beam
	if( IsValid( effectFriendly ) )
		EffectStop( effectFriendly )
	
	// setup evac trigger
	entity trigger = CreateEntity( "trigger_cylinder" )
	// increased from default
	trigger.SetRadius( 250 )
	trigger.SetAboveHeight( 150 )
	trigger.SetBelowHeight( 150 )
	trigger.SetOrigin( dropship.GetOrigin() )
	trigger.SetParent( dropship, "ORIGIN" )
	DispatchSpawn( trigger )
	// have to do this inline since we capture the completionCallback
	trigger.SetEnterCallback( void function( entity trigger, entity player ) : ( canBoardCallback, dropship ) 
	{ 	
		if ( !player.IsPlayer() || !IsAlive( player ) || player.IsTitan() || player.ContextAction_IsBusy() || !canBoardCallback( dropship, player ) || PlayerInDropship( player, dropship ) )
			return
		
		thread AddPlayerToEvacDropship( dropship, player )
	})
	
	dropship.s.evacTrigger <- trigger
		
	float waitStartTime = Time()
	while ( Time() - waitStartTime < waitTime )
	{
		if ( shouldLeaveEarlyCallback( dropship ) )
			break
			
		WaitFrame()
	}

	// fly out sound
	StopSoundOnEntity( dropship, evacShip.hoverSound )
	EmitSoundOnEntity( dropship, evacShip.flyoutSound )
	
	// holster all weapons
	foreach ( entity player in dropship.s.evacSlots )
		if ( IsValid( player ) )
			player.HolsterWeapon()
	
	// fly away
	dropship.Signal( "EvacShipLeaves" )
	thread PlayAnim( dropship, "cd_dropship_rescue_side_end", evacNode )
	
	SetTeamActiveObjective( evacTeam, "EG_DropshipExtractDropshipFlyingAway" )
	SetTeamActiveObjective( GetOtherTeam( evacTeam ), "EG_StopExtractDropshipFlyingAway" )
	
	EmitSoundOnEntity(dropship, "crow_evac_warpflyout_6ch_v2_03")
	wait dropship.GetSequenceDuration( "cd_dropship_rescue_side_end" ) - WARPINFXTIME
	
	foreach ( entity player in dropship.s.evacSlots )
		if ( IsValid( player ) )
			Remote_CallFunction_NonReplay( player, "ServerCallback_PlayScreenFXWarpJump" )
	
	wait WARPINFXTIME

	dropship.kv.VisibilityFlags = 0 // prevent jetpack trails being like "dive" into ground
	WaitFrame() // better wait because we are server
	if( !IsValid( dropship ) )
		return
	thread __WarpOutEffectShared( dropship )
	
	// go to space 
	
	// hardcoded angles here are a hack, spacenode position doesn't face the planet in the skybox, for some reason
	// nvm removing for now
	//file.spaceNode.SetAngles( < 30, -75, 20 > )
	
	dropship.SetOrigin( file.spaceNode.GetOrigin() )
	dropship.SetAngles( file.spaceNode.GetAngles() )
	dropship.SetInvulnerable()
	dropship.Signal( "EvacOver" )
	thread PlayAnim( dropship, "ds_space_flyby_dropshipA", file.spaceNode )
	
	foreach( entity player in GetPlayerArray() )
	{	
		// evac-ed players only beyond this point
		if ( !PlayerInDropship( player, dropship ) )
		{
			if ( player.GetTeam() == dropship.GetTeam() )
			{
				SetPlayerActiveObjective( player, "EG_DropshipExtractFailedEscape" )
				SetPlayerChallengeEvacState( player, 2 )
			}
				
			continue
		}
		
		SetPlayerActiveObjective( player, "EG_DropshipExtractSuccessfulEscape" )

		dropship.kv.VisibilityFlags = ENTITY_VISIBLE_TO_FRIENDLY
		
		// skybox
		player.SetSkyCamera( GetEnt( SKYBOXSPACE ) )
		Remote_CallFunction_NonReplay( player, "ServerCallback_DisableHudForEvac" )
		Remote_CallFunction_NonReplay( player, "ServerCallback_SetClassicSkyScale", dropship.GetEncodedEHandle(), 0.7 )
		Remote_CallFunction_NonReplay( player, "ServerCallback_SetMapSettings", 4.0, false, 0.4, 0.125 )
		SetPlayerChallengeEvacState( player, 1 )		
		// display player [Evacuated] in killfeed
		foreach ( entity otherPlayer in GetPlayerArray() )
			Remote_CallFunction_NonReplay( otherPlayer, "ServerCallback_EvacObit", player.GetEncodedEHandle() )
	}

	// award player score to evacing team
	int evacCount = 0
	array<entity> evacingPlayers = GetPlayerArrayOfTeam( dropship.GetTeam() ) // all players that are supposed to evac in the dropship

	// count how many players are in the dropship
	foreach ( entity player in evacingPlayers )
	{
		if ( !PlayerInDropship( player, dropship ) )
			continue
		
		evacCount++
	}

	bool allEvac = evacCount == evacingPlayers.len()

	foreach(entity player in evacingPlayers)
	{
		if ( !PlayerInDropship( player, dropship ) )
			continue

		AddPlayerScore( player, "HotZoneExtract" )
		UpdatePlayerStat( player, "misc_stats", "evacsSurvived" )

		if ( allEvac )
			AddPlayerScore( player, "TeamBonusFullEvac" )
	}

	// sole survivor (but not the only one on the team)
	if ( evacCount == 1 && !allEvac )
	{
		// we can assume there is one player in the array because otherwise evacCount wouldn't be 1
		AddPlayerScore( evacingPlayers[0], "SoleSurvivor" )
	}
}

void function AddPlayerToEvacDropship( entity dropship, entity player ) 
{
	int slot = RandomInt( dropship.s.evacSlots.len() )
	for ( int i = 0; i < dropship.s.evacSlots.len(); i++ )
	{
		if ( !IsValid( dropship.s.evacSlots[ slot ] ) )
		{
			dropship.s.evacSlots[ slot ] = player
			break
		}
	
		slot = ( slot + 1 ) % expect int( dropship.s.evacSlots.len() )
	}
	
	// no slots available
	if ( !PlayerInDropship( player, dropship ) )
		return

	UpdatePlayerStat( player, "misc_stats", "evacsAttempted" )

	// need to cancel if the dropship dies
	dropship.EndSignal( "OnDeath", "OnDestroy" )
	player.EndSignal( "OnDeath", "OnDestroy" )

	player.SetInvulnerable()
	player.UnforceCrouch()
	player.ForceStand()

	FirstPersonSequenceStruct fp
	//fp.firstPersonAnim = EVAC_EMBARK_ANIMS_1P[ slot ]
	fp.thirdPersonAnim = EVAC_EMBARK_ANIMS_3P[ slot ]
	fp.attachment = "RESCUE"
	fp.teleport = true
	fp.thirdPersonCameraAttachments = [ "VDU" ] // this seems wrong, firstperson anim has better angles, but no head
	
	EmitSoundOnEntityOnlyToPlayer( player, player, SHIFTER_START_SOUND_3P )
	// should play SHIFTER_START_SOUND_1P when they actually arrive in the ship i think, unsure how this is supposed to be done
	PlayPhaseShiftDisappearFX( player )
	FirstPersonSequence( fp, player, dropship )
	
	FirstPersonSequenceStruct idleFp
	idleFp.firstPersonAnimIdle = EVAC_IDLE_ANIMS_1P[ slot ]
	idleFp.thirdPersonAnimIdle = EVAC_IDLE_ANIMS_3P[ slot ]
	idleFp.attachment = "RESCUE"
	idleFp.teleport = true
	idleFp.hideProxy = true
	idleFp.viewConeFunction = ViewConeWide  
		
	thread FirstPersonSequence( idleFp, player, dropship )
	ViewConeWide( player ) // gotta do this after for some reason, adding it to idleFp does not work for some reason
}

bool function PlayerInDropship( entity player, entity dropship )
{
	// couldn't get "player in dropship.s.evacSlots" to work for some reason, likely due to static typing?
	foreach ( entity dropshipPlayer in dropship.s.evacSlots )
		if ( dropshipPlayer == player )
			return true
			
	return false
}

void function EvacDropshipKilled( entity dropship, var damageInfo )
{
	file.evacDead = true
	array<entity> survivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR )
	foreach ( entity player in dropship.s.evacSlots )
	{
		if ( IsValid( player ) && IsAlive( player ) )
		{
			player.ClearParent()
			player.Die( DamageInfo_GetAttacker( damageInfo ), DamageInfo_GetWeapon( damageInfo ), { damageSourceId = eDamageSourceId.evac_dropship_explosion, scriptType = DF_GIB } )
		}
	}
}

bool function CanPlayerBoardEvac( entity dropship, entity player )
{
	// can't board a dropship on a different team
	if ( dropship.GetTeam() != player.GetTeam() )
		return false
	
	// check if there are any free slots on the dropship, if there are then they can board
	foreach ( entity player in dropship.s.evacSlots )
		if ( !IsValid( player ) )
			return true
	
	// no empty slots
	return false
}

bool function EvacEpilogueShouldLeaveEarly( entity dropship )
{
	int numEvacing
	foreach ( entity player in dropship.s.evacSlots )
		if ( IsValid( player ) )
			numEvacing++

	return GetPlayerArrayOfTeam_Alive( dropship.GetTeam() ).len() == numEvacing || numEvacing == dropship.s.evacSlots.len()
}

void function EvacEpilogueCompleted( entity dropship )
{
	wait 3.0
	
	if( !IsValid(dropship) && !IsAlive(dropship) )
		return

	array<entity> survivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR )
	int numEvacing = 0
	foreach ( entity player in survivors) {
		if ( !PlayerInDropship( player, dropship ) ) {
			InfectPlayer( player, player )
			RespawnInfected( player )
		}
	}
	wait 3.0
	foreach ( entity player in dropship.s.evacSlots )
		if ( IsValid( player ) )
			numEvacing++

	if ( numEvacing != 0 )
		SetWinner(INFECTION_TEAM_SURVIVOR)
	else
		SetWinner(INFECTION_TEAM_INFECTED)

	foreach ( entity player in GetPlayerArray() )
		ScreenFadeToBlackForever( player, 2.0 )
}

// nessie functions
void function CheckIfAnyPlayerLeft( int evacTeam )
{
	wait GAME_EPILOGUE_PLAYER_RESPAWN_LEEWAY
	float startTime = Time()

	OnThreadEnd(
		function() : ( evacTeam )
		{
			SetTeamActiveObjective( evacTeam, "EG_DropshipExtractEvacPlayersKilled" )
			SetTeamActiveObjective( GetOtherTeam( evacTeam ), "EG_StopExtractEvacPlayersKilled" )
			thread EvacEpilogueCompleted( null )
            // score for killing the entire evacing team
			foreach ( entity player in GetPlayerArray() )
			{
				if ( player.GetTeam() == evacTeam )
					continue

				AddPlayerScore( player, "TeamBonusKilledAll")
			}
		}
	)
	while( true )
	{
		if( GetPlayerArrayOfTeam_Alive( evacTeam ).len() == 0 )
			break
		if( GetGameState() == eGameState.Postmatch )
			return
		WaitFrame()
	}	
}

void function DropShipTempHide( entity dropship )
{
	dropship.kv.VisibilityFlags = 0 // or it will still shows the jetpack fxs
	HideName( dropship )
	wait 0.46
	if( IsValid( dropship ) )
	{
		dropship.kv.VisibilityFlags = ENTITY_VISIBLE_TO_EVERYONE
		ShowName( dropship )
	}
}

EvacShipSetting function GetEvacShipSettingByTeam( int team )
{
	EvacShipSetting tempSetting
	if( team == TEAM_IMC )
	{
		tempSetting.shipModel = $"models/vehicle/goblin_dropship/goblin_dropship_hero.mdl"
		tempSetting.flyinSound = "Goblin_IMC_Evac_Flyin"
		tempSetting.hoverSound = "Goblin_IMC_Evac_Hover"
		tempSetting.flyoutSound = "Goblin_IMC_Evac_FlyOut"
	}
	if( team == TEAM_MILITIA )
	{
		tempSetting.shipModel = $"models/vehicle/crow_dropship/crow_dropship_hero.mdl"
		tempSetting.flyinSound = "Crow_MCOR_Evac_Flyin"
		tempSetting.hoverSound = "Crow_MCOR_Evac_Hover"
		tempSetting.flyoutSound = "Crow_MCOR_Evac_Flyout"
	}
	return tempSetting
}

void function EvacDropshipDamaged( entity evacShip, var damageInfo )
{
	int damageSourceID = DamageInfo_GetDamageSourceIdentifier( damageInfo )
	if( damageSourceID == damagedef_nuclear_core )
		DamageInfo_SetDamage( damageInfo, DamageInfo_GetDamage( damageInfo )/3 )
}

void function WarpInEffectEvacShip( entity dropship )
{
    dropship.EndSignal( "OnDestroy" )
	float sfxWait = 0.1
	float totalTime = WARPINFXTIME
	float preWaitTime = 0.16 // give it some time so it's actually playing anim, and we can get it's "origin" attatch
	string sfx = "dropship_warpin"

	wait preWaitTime

	int attach = dropship.LookupAttachment( "origin" )
	vector origin = dropship.GetAttachmentOrigin( attach )
	vector angles = dropship.GetAttachmentAngles( attach )

	entity fx = PlayFX( FX_GUNSHIP_CRASH_EXPLOSION_ENTRANCE, origin, angles )
	fx.FXEnableRenderAlways()
	fx.DisableHibernation()

	wait sfxWait
	EmitSoundAtPosition( TEAM_UNASSIGNED, origin, sfx )

	wait totalTime - sfxWait
}

//======================================================================================================
//
// ██████   █████  ███    ██ ███████ ██      ███████ 
// ██   ██ ██   ██ ████   ██ ██      ██      ██      
// ██████  ███████ ██ ██  ██ █████   ██      ███████ 
// ██      ██   ██ ██  ██ ██ ██      ██           ██ 
// ██      ██   ██ ██   ████ ███████ ███████ ███████
//
//======================================================================================================

void function InfectionAddPanelSpawns( array<vector> positionsAndOrigins )
{
	for ( int i = 0; i < positionsAndOrigins.len(); i += 2 )
	{
		Point spawnPoint
		spawnPoint.origin = positionsAndOrigins[ i ]
		spawnPoint.angles = positionsAndOrigins[ i + 1 ]
		
		file.panelSpawns.append( spawnPoint )
	}
}

void function SpawnPanelsForLevel()
{
	int panelId
	foreach ( Point panelSpawn in file.panelSpawns )
	{
		entity panel = CreatePanel( panelSpawn.origin, panelSpawn.angles )	
		panel.s.panelId <- panelId++
		SetTeam( panel, INFECTION_TEAM_INFECTED )
	}
}

void function InfectionAddProp( asset model, array<vector> positionsAndOrigins)
{
	for ( int i = 0; i < positionsAndOrigins.len(); i += 2 )
	{
		entity prop = CreateEntity( "prop_control_panel" )
		prop.SetValueForModelKey( model )
		prop.SetOrigin( positionsAndOrigins[ i ] )
		prop.SetAngles( positionsAndOrigins[ i + 1 ] )
		prop.kv.solid = SOLID_VPHYSICS
		DispatchSpawn( prop )
	
		prop.SetModel( model )
	}
}

entity function CreatePanel( vector origin, vector angles )
{
	entity panel = CreateEntity( "prop_control_panel" )
	panel.SetValueForModelKey( $"models/communication/terminal_usable_imc_01.mdl" )
	panel.SetOrigin( origin )
	panel.SetAngles( angles )
	panel.kv.solid = SOLID_VPHYSICS
	DispatchSpawn( panel )
	
	panel.SetModel( $"models/communication/terminal_usable_imc_01.mdl" )
	panel.s.scriptedPanel <- true
	
	// HACK: need to use a custom useFunction here as control panel exposes no way to get the player's position before hacking it, or a way to run code before the hacking animation actually starts
	panel.s.startOrigin <- < 0, 0, 0 >
	panel.useFunction = InfectionControlPanelCanUse
	
	SetControlPanelUseFunc( panel, InfectionOnPanelHacked )
	
	Highlight_SetNeutralHighlight( panel, "sp_enemy_pilot" )
	
	return panel
}

function InfectionControlPanelCanUse( playerUser, controlPanel )
{
	// just run ControlPanel_CanUseFunction, but save hacking player's origin to controlPanel.s.startOrigin beforehand
	expect entity( playerUser )
	expect entity( controlPanel )
	controlPanel.s.startOrigin <- playerUser.GetOrigin()

	foreach ( entity weapon in playerUser.GetOffhandWeapons() ) {
		if ( weapon.GetWeaponClassName() == "mp_ability_burncardweapon" || weapon.GetWeaponClassName() == "mp_ability_turretweapon" || weapon.GetWeaponClassName() == "mp_weapon_frag_drone" ) {
			return false
		}
	}
		
	return ControlPanel_CanUseFunction( playerUser, controlPanel )
}

// control panel code isn't very statically typed, pain
function InfectionOnPanelHacked( panel, player )
{
	expect entity( panel )
	expect entity( player )
		
	print( panel + " was hacked by " + player )
	
	player.SetPlayerGameStat( PGS_ASSAULT_SCORE, player.GetPlayerGameStat( PGS_ASSAULT_SCORE ) + 1 )

	array<entity> dropPodSpawns = GetEntArrayByClass_Expensive( "info_spawnpoint_droppod_start" )
	if (dropPodSpawns.len() != 0) {
		entity closestNode = GetClosest( dropPodSpawns, player.GetOrigin() )
		if (RandomInt(10) == 0) {
			if (player.GetTeam() == INFECTION_TEAM_SURVIVOR) {
				thread Infection_SpawnDropPodAlly( closestNode.GetOrigin(), <0, 0, 0>, INFECTION_TEAM_SURVIVOR, "npc_spectre", SquadFollow, 0, player )
			} else {
				thread Infection_SpawnDropPodAlly( closestNode.GetOrigin(), <0, 0, 0>, INFECTION_TEAM_INFECTED, "npc_prowler", SquadFollow, 0, player )
			}
		}
		else {
			if (player.GetTeam() == INFECTION_TEAM_SURVIVOR) {
				BurnMeter_GiveRewardDirect( player, file.highburnreward.getrandom() )
			}
		}
	}
	else {
		if (player.GetTeam() == INFECTION_TEAM_SURVIVOR) {
			BurnMeter_GiveRewardDirect( player, file.highburnreward.getrandom() )
		}
	}

	SetTeam( panel, TEAM_UNASSIGNED )

	thread InfectionOnPanelHacked_Delayed( panel, player )


	// TODO: figure out if we want to do this clientsided or serversided (basically is MAD good enough/going to stay)
	// foreach ( entity otherPlayer in GetPlayerArray() )
	// 	Remote_CallFunction_NonReplay( otherPlayer, "ServerCallback_InfectionPanelHacked", panel.GetEncodedEHandle(), panel.s.panelId, player.GetEncodedEHandle() )
}

function InfectionOnPanelHacked_Delayed( panel, player )
{
	wait 30
	expect entity( panel )
	expect entity( player )
		
	SetTeam( panel, INFECTION_TEAM_SURVIVOR )
	EmitSoundOnEntity( panel, "Coop_AmmoBox_Close" ) // might change, it's a little quiet
}

void function ResetPanels()
{
	foreach ( entity panel in GetAllControlPanels() )
	{
		panel.SetUsableByGroup( "enemies pilot" )
		SetTeam( panel, INFECTION_TEAM_SURVIVOR )
	}
}

void function ShouldPanelsBeUsable()
{
	int numInfected = GetPlayerArrayOfTeam( INFECTION_TEAM_INFECTED ).len()
	int numSurvivors = GetPlayerArrayOfTeam( INFECTION_TEAM_SURVIVOR ).len()
	
	foreach ( entity panel in GetAllControlPanels() )
	{
		if ( numInfected >= numSurvivors )
			panel.SetUsableByGroup( "friendlies pilot" )
		else
			panel.UnsetUsable()
	}
}

void function SquadFollow( array<entity> guys, entity owner )
{
	int team = guys[0].GetTeam()
	// show the squad enemy radar
	int goalradius = guys[0].GetClassName() == "npc_stalker" ? 1600 : 800
	array<entity> players = GetPlayerArrayOfEnemies( team )
	foreach ( entity guy in guys )
	{
		if ( IsAlive( guy ) )
		{
			foreach ( player in players )
				guy.Minimap_AlwaysShow( 0, player )
		}
	}
	
	vector point = owner.GetOrigin()
	
	// Setup AI, first assault point
	foreach ( guy in guys )
	{
        if ( IsAlive( guy ) ) {
            guy.EnableNPCFlag( NPC_ALLOW_PATROL | NPC_ALLOW_INVESTIGATE | NPC_ALLOW_HAND_SIGNALS | NPC_ALLOW_FLEE | NPC_STAY_CLOSE_TO_SQUAD | NPC_CROUCH_COMBAT )
            guy.AssaultPoint( point )
            guy.AssaultSetGoalRadius( goalradius ) // 1600 is minimum for npc_stalker, works fine for others

		//thread AITdm_CleanupBoredNPCThread( guy )
        }
	}
	
	// Every 5 - 15 secs change AssaultPoint
	while ( true )
	{	
		if ( !IsValid( owner ) || owner.GetTeam() != team ) {
			if ( GetPlayerArrayOfTeam( team ).len() == 0 ) {
				return // round is over, no need to cleanup squad, maybe make them explode or smth if its a problem
			}
			owner = GetPlayerArrayOfTeam( team ).getrandom()
		}

		foreach ( guy in guys )
		{
			// Check if alive
			if ( !IsAlive( guy ) )
			{
				guys.removebyvalue( guy )
				continue
			}
			// Stop func if our squad has been killed off
			if ( guys.len() == 0 )
				return
		}
		
		point = owner.GetOrigin()

		if ( IsValid(file.queen) )
			point = file.queen.GetOrigin()
		
		foreach ( guy in guys )
		{
			if ( IsAlive( guy ) )
				guy.AssaultPoint( point )
		}

		wait RandomFloatRange(3.0, 5.0)
	}
}

void function RateSpawnpoints_Infection( int checkClass, array<entity> spawnpoints, int team, entity player )
{	
	foreach ( entity spawnpoint in spawnpoints )
	{
		float currentRating = 0.0
		
		// Gather friendly scoring first to give positive rating first
		currentRating += spawnpoint.NearbyAllyScore( team, "ai" )
		currentRating += spawnpoint.NearbyAllyScore( team, "titan" )
		currentRating += spawnpoint.NearbyAllyScore( team, "pilot" )
		
		// Enemies then subtract that rating ( Values already returns negative, so no need to apply subtract again )
		currentRating += spawnpoint.NearbyEnemyScore( team, "ai" )
		currentRating += spawnpoint.NearbyEnemyScore( team, "titan" )
		currentRating += spawnpoint.NearbyEnemyScore( team, "pilot" )
		
		if ( spawnpoint == player.p.lastSpawnPoint ) // Reduce the rating of the spawn point used previously
			currentRating += GetConVarFloat( "spawnpoint_last_spawn_rating" )

		if ( IsValid( file.queen ) ) {
			currentRating = 1.0 - ( Distance2D( spawnpoint.GetOrigin(), file.queen.GetOrigin() ) / MAP_EXTENTS )
		}
		
		spawnpoint.CalculateRating( checkClass, team, currentRating, currentRating * 0.25 )
	}
	// foreach ( entity spawnpoint in spawnpoints )
    // {
    //     spawnpoint.NearbyEnemyScore( team, "ai" )

    //     if( !IsValid( file.queen ) )
    //         break
        
    //     float currentRating = 1.0 - ( Distance2D( spawnpoint.GetOrigin(), file.queen.GetOrigin() ) / MAP_EXTENTS )

    //     // if ( spawnpoint == player.p.lastSpawnPoint ) // Reduce the rating of the spawn point used previously
    //     //     currentRating += GetConVarFloat( "spawnpoint_last_spawn_rating" )

    //     spawnpoint.CalculateRating( checkClass, team, currentRating, currentRating * 0.25 )
    // }
}

// Yoinked from EladNLG's rougelike mod
int function AddClipToWeapon( entity player, entity weapon )
{
	int ammoPerClip = weapon.GetWeaponPrimaryClipCountMax()
	int gainedAmmo = 0

	switch ( weapon.GetWeaponInfoFileKeyField( "fire_mode" ) )
	{
		case "offhand_hybrid":
		case "offhand":
		case "offhand_instant":

			// offhand weapons typically cant store ammo, so refill the current clip
			if ( ammoPerClip > 0 )
			{
				int primaryClipCount = weapon.GetWeaponPrimaryClipCount()
				weapon.SetWeaponPrimaryClipCount( ammoPerClip )
				gainedAmmo = weapon.GetWeaponPrimaryClipCount() - primaryClipCount
			}
			break

		default:
			int primaryAmmoCount = weapon.GetWeaponPrimaryAmmoCount()
			// this weapon has off-clip ammo storage, so add ammo to storage
			int stockpile = player.GetWeaponAmmoStockpile( weapon )
			weapon.SetWeaponPrimaryAmmoCount( primaryAmmoCount + ammoPerClip )
			gainedAmmo = player.GetWeaponAmmoStockpile( weapon ) - stockpile
			break
	}

	return gainedAmmo
}
