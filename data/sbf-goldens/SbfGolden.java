import java.io.File;
import java.util.*;
import megamek.common.Entity;
import megamek.common.EquipmentType;
import megamek.common.MekFileParser;
import megamek.common.alphaStrike.AlphaStrikeElement;
import megamek.common.alphaStrike.ASDamage;
import megamek.common.alphaStrike.ASDamageVector;
import megamek.common.alphaStrike.conversion.ASConverter;
import megamek.client.ui.clientGUI.calculationReport.DummyCalculationReport;
import megamek.common.strategicBattleSystems.SBFUnit;
import megamek.common.strategicBattleSystems.SBFUnitConverter;

/**
 * Dumps golden SBF unit-conversion fixtures: for each fixture, the Alpha Strike element INPUTS
 * (so neurohelmet can feed identical inputs to its own convert_unit) and the SBFUnit OUTPUT that
 * MegaMek's SBFUnitConverter produces. Emits a JSON array to stdout.
 */
public class SbfGolden {
    static final String MEKS = "/Users/nate/dev/MegaMek/data/mekfiles/";

    // Each fixture: name, then the unit files (relative to MEKS) that form one SBF Unit.
    static final String ATLAS = "meks/3039u/Atlas AS7-D.mtf";
    static final String LOCUST = "meks/3039u/Locust LCT-1V.mtf";
    static final String DEMO = "vehicles/Rec Guides ilClan/Vol 25/Demolisher Heavy Tank (Arrow IV).blk";
    static final String SPARROW = "fighters/TRO3039u/Sparrowhawk SPR-H5.blk";
    static final String FOOT = "infantry/TW/IS Squads/Foot Squad (Rifle).blk";
    static Object[][] FIXTURES = {
        { "Atlas Lance", new String[]{ ATLAS, ATLAS, ATLAS, ATLAS } },
        { "Locust Lance", new String[]{ LOCUST, LOCUST, LOCUST, LOCUST } },
        { "Mixed", new String[]{ ATLAS, ATLAS, LOCUST, LOCUST } },
        { "Demolisher (all-turret)", new String[]{ DEMO, DEMO, DEMO, DEMO } },
        { "Aero Flight", new String[]{ SPARROW, SPARROW, SPARROW, SPARROW } },
        { "Infantry", new String[]{ FOOT, FOOT, FOOT, FOOT } },
    };

    static String esc(String s) { return s.replace("\\", "\\\\").replace("\"", "\\\""); }

    static String dmg(ASDamage d) { return "\"" + esc(d.toString()) + "\""; }  // "5", "0*", "-"

    public static void main(String[] args) throws Exception {
        EquipmentType.initializeTypes();
        StringBuilder out = new StringBuilder("[\n");
        for (int fi = 0; fi < FIXTURES.length; fi++) {
            String name = (String) FIXTURES[fi][0];
            String[] files = (String[]) FIXTURES[fi][1];
            List<AlphaStrikeElement> elems = new ArrayList<>();
            StringBuilder elemJson = new StringBuilder();
            for (int i = 0; i < files.length; i++) {
                Entity ent = new MekFileParser(new File(MEKS + files[i])).getEntity();
                AlphaStrikeElement e = ASConverter.convert(ent);
                elems.add(e);
                ASDamageVector sd = e.getStandardDamage();
                if (i > 0) elemJson.append(",\n");
                elemJson.append("      {")
                    .append("\"name\":\"").append(esc(e.getName())).append("\",")
                    .append("\"tp\":\"").append(esc(e.getASUnitType().toString())).append("\",")
                    .append("\"size\":").append(e.getSize()).append(",")
                    .append("\"mv\":\"").append(esc(e.getMovementAsString())).append("\",")
                    .append("\"armor\":").append(e.getFullArmor()).append(",")
                    .append("\"structure\":").append(e.getFullStructure()).append(",")
                    .append("\"dmgS\":").append(dmg(sd.S)).append(",")
                    .append("\"dmgM\":").append(dmg(sd.M)).append(",")
                    .append("\"dmgL\":").append(dmg(sd.L)).append(",")
                    .append("\"dmgE\":").append(dmg(sd.E)).append(",")
                    .append("\"ov\":").append(e.getOV()).append(",")
                    .append("\"th\":").append(e.getThreshold()).append(",")
                    .append("\"pv\":").append(e.getBasePointValue()).append(",")
                    .append("\"skill\":").append(e.getSkill()).append(",")
                    .append("\"specials\":\"").append(esc(e.getSpecialsDisplayString("|", e))).append("\"")
                    .append("}");
            }
            SBFUnit u = new SBFUnitConverter(elems, name, new DummyCalculationReport()).createSbfUnit();
            ASDamageVector ud = u.getDamage();
            if (fi > 0) out.append(",\n");
            out.append("  {\n")
               .append("    \"name\":\"").append(esc(name)).append("\",\n")
               .append("    \"elements\":[\n").append(elemJson).append("\n    ],\n")
               .append("    \"unit\":{")
               .append("\"type\":\"").append(esc(u.getType().toString())).append("\",")
               .append("\"size\":").append(u.getSize()).append(",")
               .append("\"mv\":").append(u.getMovement()).append(",")
               .append("\"mvCode\":\"").append(esc(u.getMovementCode())).append("\",")
               .append("\"jump\":").append(u.getJumpMove()).append(",")
               .append("\"trspMv\":").append(u.getTrspMovement()).append(",")
               .append("\"trspCode\":\"").append(esc(u.getTrspMovementCode())).append("\",")
               .append("\"tmm\":").append(u.getTmm()).append(",")
               .append("\"armor\":").append(u.getArmor()).append(",")
               .append("\"dmgS\":").append(dmg(ud.S)).append(",")
               .append("\"dmgM\":").append(dmg(ud.M)).append(",")
               .append("\"dmgL\":").append(dmg(ud.L)).append(",")
               .append("\"dmgE\":").append(dmg(ud.E)).append(",")
               .append("\"skill\":").append(u.getSkill()).append(",")
               .append("\"pv\":").append(u.getPointValue()).append(",")
               .append("\"specials\":\"").append(esc(u.getSpecialAbilities().getSpecialsDisplayString("|", u))).append("\"")
               .append("}\n  }");
        }
        out.append("\n]\n");
        System.out.println(out);
    }
}
