"""Art direction for the united overworld.

Coordinates are percent of the complete atlas; local outlines are map units.
A district is a rigidly relocated piece of authored landscape, including its
paint, entrances, terraces, forest edges and shores. Borders sit outside the
architecture. Connecting routes, reserves and outlooks are drawn explicitly.
"""
from dataclasses import dataclass
import numpy as np


@dataclass(frozen=True)
class District:
    name: str
    anchor: int
    target: tuple
    members: tuple
    outline: tuple
    intent: str
    origin: tuple | None = None


def cut_box(left, top, right, bottom, cut=250):
    return ((left+cut,top),(right-cut,top),(right,top+cut),
            (right,bottom-cut),(right-cut,bottom),(left+cut,bottom),
            (left,bottom-cut),(left,top+cut))


DISTRICTS = [
    District('Iselia woodland basin',2,(17,49),(1,2,11,12,13,14,53),
             ((-6500,-4900),(-3900,-6500),(1500,-5400),(3200,-1600),(2700,3200),(-4500,2700),(-6500,-500)),
             'Keep Dirk, the forest gates and Martel on their original wooded terraces. Iselia opens east onto the fusion scar.'),
    District('Luin river garden',7,(71,37.5),(7,43,44),cut_box(-2600,-2300,2600,2300,700),
             'A compact river town at the foot of the northern range; three entrances share one continuous river landscape.'),
    District('Asgard stair country',6,(80,50),(6,),cut_box(-1800,-1200,1700,2800,450),
             'The entire stepped town, escarpment and south approach remain one composition.'),
    District('Hima crag',8,(54,52),(8,),cut_box(-2300,-2600,2300,2200,650),
             'A hilltop silhouette above the inner sea, reached by a switchback from the southern meadow.'),
    District('Palmacosta bay',5,(31,61),(5,),cut_box(-2300,-2300,3000,2600,500),
             'The working port sits in its own native bay, with a long view back toward Ossa.'),
    District('Izoold fishing shore',4,(19,68),(4,),cut_box(-2000,-1900,2200,1900,500),
             'A small southern fishing coast, deliberately separated from the larger trading port.'),
    District('Ossa saddle',17,(24,62),(17,18),cut_box(-1700,-2100,2600,1300,450),
             'Both ends of the trail keep their original mountain saddle; the road bends around its west shoulder.'),
    District('Lake Umacy hollow',27,(24,55),(27,),cut_box(-1700,-1500,1700,1700,500),
             'A quiet destination off the main road, tucked behind the eastern woodland.'),
    District('Hakonesia pass',41,(15,32),(41,42),cut_box(-1900,-2300,2000,1500,500),
             'A real paired pass on the long northern peninsula.'),
    District('Tower of Mana headland',23,(14,18),(23,),cut_box(-1800,-1700,1800,1600,450),
             'The old tower terminates the northern pilgrimage road.'),
    District('Northern ranch',22,(13,25),(22,),cut_box(-1100,-1000,1200,1000,250),
             'A walled clearing below the tower, approached obliquely rather than placed on the road axis.'),
    District('Eastern ranch',20,(30,50),(20,),cut_box(-1300,-1400,1500,1400,300),
             'A fortified outlier on the eastern Euclid shelf.'),
    District('Thoda landing',29,(37,64),(29,),cut_box(-1000,-1000,1100,1100,250),
             'The landing faces the offshore geyser instead of sitting inland.'),
    District('Thoda volcanic isle',19,(43,57),(19,),cut_box(-1400,-1300,1400,1400,400),
             'An isolated, recognizable island excursion in the inner sea.'),
    District('Sylvarant tower isle',24,(41,50),(24,),cut_box(-1400,-1500,1400,1500,350),
             'One of the two old-world axes, seen across water from Euclid.'),
    District('Meltokio crown terrace',257,(57,70),(257,),cut_box(-2500,-2300,2400,2300,500),
             'The capital keeps its native retaining ground and broad southern approach.'),
    District('Grand bridge estuary',268,(61,58),(268,269,264),
             ((-1900,-1300),(7000,-8500),(12400,-8000),(12600,-4000),(2300,2400),(-1500,1700)),
             'Keep the complete bridge, its water and both endpoints together. It becomes the long diagonal spine of Midgard.'),
    District('Sybak scholarly coast',258,(64,38),(258,),cut_box(-2000,-1800,2200,2100,450),
             'A low, open campus with a sheltered shore, west of the great forest.'),
    District('Gaoracchia and Mizuho',283,(80,34),(283,284,285,286,259),
             ((-2600,-3400),(2200,-4100),(6200,-2200),(6500,4300),(2300,5700),(-1800,2900)),
             'All four forest gates and Mizuho keep the original connected forest, clearings and paths.'),
    District('Ozette and Altessa',260,(40,75),(260,261),cut_box(-1900,-1600,1900,2600,450),
             'A village and workshop share one gentle woodland basin.'),
    District('Heimdall and Ymir',265,(48,75.5),(265,278),cut_box(-2500,-2100,2100,3600,600),
             'The elven village remains nested in its native forest, with Ymir as its actual southern threshold.'),
    District('Flanoir snow bowl',262,(43,14),(262,),cut_box(-2800,-2400,3100,2900,650),
             'A town in a sheltered snow bowl, rather than a model dropped on a snowy slope.'),
    District('Ice temple cirque',273,(49,24),(273,),cut_box(-1800,-1700,1900,2000,500),
             'The pilgrimage leaves Flanoir through a low pass and climbs into an enclosed cirque.'),
    District('Katz sanctuary',9,(22,91),(9,),cut_box(-2400,-1100,2500,2500,500),
             'An intimate southern sanctuary; the native terraces and great tree remain intact.'),
    District('Freyland oasis country',3,(82,76),(3,16,15,54),
             ((-5600,-3000),(-1200,-4300),(5900,-3300),(6500,2200),(2300,4200),(-4900,3900)),
             'Triet, its oasis, the ruins and base form a complete desert district with native dunes and ravines.'),
    District('Altamira resort shore',263,(86,62),(263,),cut_box(-2000,-1900,2300,2100,450),
             'The resort faces a real coast and keeps its native shoreline architecture.'),
    District('Toize mining cape',272,(91,52),(272,),cut_box(-1800,-1800,2000,2100,450),
             'A rugged mining cape across the channel from the resort.'),
    District('Earth temple foothill',275,(77,63),(275,),cut_box(-1800,-1900,1800,1900,500),
             'A rocky counterpart to the oasis, above the northwest Freyland approach.'),
    District('Darkness escarpment',276,(53,61),(276,),cut_box(-2100,-2400,1900,2200,450),
             'Keep the native high escarpment and cave approach, with a small dead-end route.'),
    District('Lightning tableland',274,(89,25),(274,),cut_box(-2100,-2200,2100,2200,450),
             'An exposed northern tableland, separated from the forest valleys.'),
    District('Balacruf wind cape',21,(94,36),(21,),cut_box(-1600,-1800,1700,1800,400),
             'A windward sea cliff at the far eastern edge.'),
    District('Tethealla base',271,(91,44),(271,),cut_box(-1600,-1600,1800,1800,400),
             'A military approach below the wind cape.'),
    District('Fooji ridge',267,(61,79),(267,),cut_box(-2100,-2200,2200,2300,500),
             'A separate mountain destination beyond the capital, framing its southern horizon.'),
    District('Latheon garden gorge',279,(54.5,85.5),(279,),cut_box(-2300,-2200,2200,2400,550),
             'An offshore garden gorge, with enough breathing room to read as a destination.'),
    District('Tethealla tower rise',280,(66,70),(280,),cut_box(-1600,-1700,1700,1800,450),
             'The second old-world axis stands east of the capital.'),
    District('Hot spring retreat',288,(76,20),(288,),cut_box(-1500,-1600,1700,1600,450),
             'A pocket of calm at the foot of the northern mountains.'),
    District('Southeast abbey',291,(72.5,68),(291,),cut_box(-1500,-1500,1500,1600,350),
             'A quiet coastal abbey on the road into the sands.'),
    District('Otherworld gate',305,(92,16),(305,),cut_box(-1700,-1600,1800,1800,450),
             'A remote threshold beyond the lightning heights.'),
    District('Linkite promontory',61,(37,69),(61,),cut_box(-1300,-1300,1400,1400,350),
             'A single great tree marks the western entrance to the southern woodland.'),
    District('Island ranch',25,(69,78),(25,),cut_box(-1500,-1500,1600,1600,400),
             'An isolated fortified island off the western sands.'),
    District('Exire anchorage',266,(58.5,39),(266,),cut_box(-1800,-1700,1800,1700,450),
             'The floating town retains its original elevation above the inner sea.'),
    District('Mysterious southern temple',277,(29,89),(277,),cut_box(-1700,-1700,1700,1800,450),
             'An old-world ruin on a newly separated southern shard.'),
    District('Windward landing',256,(94.9,41.4),(),cut_box(-1000,-1000,1100,1100,250),
             'A quiet northern-facing wharf below Balacruf, joining the eastern coast road.',(42750,14800)),
    District('Latheon ferry cove',256,(61,87),(),cut_box(-1100,-1000,1200,1100,250),
             'A sheltered southern landing on the garden island, preserving the original pier and pilings.',(53550,55650)),
    District('Freyland east landing',256,(92,81),(),cut_box(-1000,-1100,1100,1100,250),
             'A green sea-facing refuge below the desert rim; its pier is a real sailing destination.',(66510,52570)),
    District('West salvation house',28,(20.5,57),(28,),cut_box(-600,-650,650,650,150),
             'A roadside pause overlooking Umacy.'),
    District('Freyland salvation house',30,(74,76),(30,),cut_box(-650,-650,650,650,150),
             'Shade and shelter before the oasis road.'),
    District('Midgard salvation house',31,(79,45),(31,),cut_box(-650,-650,650,650,150),
             'An outlook before the Asgard descent.'),
    District('Capital guidance house',290,(58,64.5),(290,),cut_box(-650,-650,650,650,150),
             'A small stop where the capital road branches toward the bridge.'),
    District('Gorge guidance house',292,(56.7,79.5),(292,),cut_box(-650,-650,650,650,150),
             'A final shelter above the crossing to Latheon.'),
]

# Deliberately placed itinerant landmarks. Every route has a destination;
# branches stop at overlooks and shrines instead of forming a spanning tree.
LOOSE_PLACES={25:(69,78),55:(73.93,41.8),57:(35.8,60),58:(81.55,67.47),60:(24.59,65.34),
              277:(29,89),1400:(16.479166666666668,50.736111111111114)}

# Named travel corridors, drawn around the native districts, not through them.
# Coordinates are percent; width is the half-width in world units.
ROUTES=[
 ('Euclid pilgrimage',75,[(15,20),(14,22),(13.4,24),(14.4,27.3),(15.8,29),(15.8,34.6),(14.3,35.3),(12.3,36.3),(10.2,37.2),(9.4,38.4),(9.55,39.66)]),
 ('Iselia eastern road',70,[(18.7,50.2),(20.7,51.4),(22.5,51.7),(24.3,52.9)]),
 ('Confluence spur',45,[(17,49.7),(16.8,50),(16.5,50.35),(16.4792,50.7361)]),
 ('Ossa road',85,[(23.9,57.4),(22.8,58.5),(22.7,60),(23.4,61.4)]),
 ('Palmacosta approach',90,[(27,61.7),(28.6,62),(29.5,61.8)]),
 ('Izoold coastal lane',65,[(22.5,63.3),(21.5,65),(20.5,65.7),(19.7,66.4)]),
 ('Umacy ranch branch',60,[(25.5,53.3),(27,52.8),(28.3,51.5),(29.4,50.5)]),
 ('Thoda ferry lane',65,[(32.8,62.3),(33.9,62.8),(34.9,63.7)]),
 ('Capital north road',95,[(57.4,66.3),(58.2,65.1),(58.6,63.3),(60.4,61.1),(61,60)]),
 ('Hima switchbacks',65,[(56.4,56.2),(55.3,55.7),(54.1,54.3),(53.5,53.9)]),
 ('Kings road',75,[(56.4,55.5),(56.8,57.5),(57.8,59),(58.2,61.4),(58.6,63.3)]),
 ('Darkness branch',50,[(56.5,63.5),(54.8,63.1),(53.8,62.2)]),
 ('Luin river road',75,[(70,41),(70.2,42.3),(73.8,42.4),(76.3,43.3),(79,45),(79,46.8),(79.6,48.3)]),
 ('Sybak western lane',75,[(65.8,40),(66.6,41.2),(68.5,41.7)]),
 ('Forest pilgrim road',60,[(77,40),(76.2,42.3),(75.6,44.6)]),
 ('Northern springs',60,[(77,22),(78.3,23.8),(80.1,25.1),(82.6,26.1),(86.1,26.6)]),
 ('Wind cape road',55,[(88.5,32),(90.4,32.7),(92.5,34.6)]),
 ('Flanoir ice pilgrimage',50,[(43.4,18.5),(44.5,20),(46.1,20.9),(47.3,22.3),(48.5,23.1)]),
 ('Oasis caravan trail',60,[(74.8,76.6),(77,76.3),(78.9,75.9),(80,76.2)]),
 ('Abbey sands road',60,[(77.1,72.2),(78.1,73.3),(79.4,73.7)]),
 ('Altamira ridge road',70,[(85.7,65.2),(84.2,67),(82.5,68.5),(80.5,69.8)]),
 ('Southern woodland path',50,[(42,76.3),(43.8,76.7),(45.2,76.4),(46.3,75.9)]),
]

# A small set of broad forest boundaries replaces thousands of seeded blobs.
# The native forest districts supply the intricate trees and clearings around
# towns; these drawn reserves shape the open land between them.
FORESTS=[
 ('Umacy eastern wood',[(22,51),(24,50),(26,51.2),(27.2,54),(26.6,56.3),(25.8,55.3),(25.4,53.1),(23.5,52.4)]),
 ('Ossa western shelter',[(19.5,56),(21.2,55),(21.7,57.6),(20.6,59.7),(21,61.1),(19.6,62),(18.4,59.3)]),
 ('Midgard western reserve',[(53,44),(55.3,43.5),(57.2,45.8),(56.7,48.1),(54,47.2),(52.2,48.5),(51.6,46.1)]),
 ('Luin northern wood',[(66.6,32.5),(69.3,32),(71,33.2),(73.2,33.6),(72.8,35.2),(70.2,35.7),(67.8,34.8)]),
 ('Asgard eastern wood',[(83.4,46),(86,44.8),(88.2,47),(87,49.7),(88,51.7),(85.1,52.8),(83.8,50.4)]),
 ('Elven western wood',[(32.8,71),(35.2,70.2),(36.9,71.2),(36.5,73.8),(34.6,75),(32.9,73.5)]),
]

# Each ridge is composed around settlements and passes, with a few readable
# peaks and spurs rather than noise-modulated chains crossing every district.
RIDGES=[
 ('Euclid northern spine',[(12.6,16),(12.5,19.3),(14.4,22),(15.8,25.7),(17,29.4),(16.2,35.9)],1250,650),
 ('Euclid southern shoulder',[(12,57),(13,60.7),(16,64.4)],1050,370),
 ('Crown western wall',[(35,8),(36.8,11),(38,15.8),(37.1,20.4)],1500,740),
 ('Crown eastern wall',[(47,8),(50.3,10.2),(52,14.9),(51.5,19.3)],1600,920),
 ('Midgard high divide',[(72.7,13),(77.2,16),(80.8,21.2),(83.7,25.5),(85.4,29.7)],1750,900),
 ('Midgard coastal spur',[(88,28.8),(89.3,31.2),(90.1,34.1),(89.3,38.2)],1150,560),
 ('Freyland rim',[(90,67),(91.5,71.7),(89.6,77.4),(87.4,83.8)],1450,590),
 ('Southern far wall',[(16,88),(19,86.9),(26.3,87.4),(31.4,90.5),(37,93.2)],1300,610),
]


def placements(original):
    native={e['id']:e for world in original.values() for e in world}
    result=dict(LOOSE_PLACES)
    for district in DISTRICTS:
        if not district.members:continue
        anchor=source_origin(district,native)
        target=np.array(district.target)*[768,576]
        for source in district.members:
            result[source]=tuple((np.array(native[source]['position'])-anchor+target)/[768,576])
    return result


DISCOVERIES=[
 (10.8,15.8),(16.8,21.5),(18,34),(8.7,40),(20,45),(23.8,47.7),(27.4,50.5),(34.5,54),
 (28,58),(33,63),(24.8,69),(16.5,69.4),(9,60),(12.5,52),(40,48),(44,54),
 (35,73),(39,79),(47.8,80),(51,76),(24,93),(33.7,95),(15,91),(41,96),
 (37,11),(48,10),(53,17),(45,25),(55,47),(52,56),(57,61),(62,68),
 (64,31),(72,31),(69,34),(73,40),(77,45),(82,47),(88.8,48),(95,39),
 (91,31),(89,22),(93,13),(81,15),(74,19),(77,28),(84,38),(86,43),
 (72,65),(78.6,67.4),(87,59),(91,65),(94,73),(87.5,82),(82,86),(76,81.8),
 (66.5,77),(61,81.5),(54,84),(53,69),(66.5,72),(44,69),(58,36),(70,52),
]


def source_origin(district,native):
    return np.array(district.origin if district.origin is not None else native[district.anchor]['position'],float)


LAND_FORMS=[
 ('Northern green saddle',120,[(15,33.8),(16,34.5),(15.5,35.7),(13.2,36.7),(11,37.2),(10.1,38.5),(10.7,39.3),(9.5,40.1),(8.2,39.7),(8.5,37.8),(10,36.4),(12.5,35.4)]),
 ('Latheon eastern shelf',130,[(57.4,84.5),(59.5,84),(61.8,85),(62.3,86.8),(61.6,87.2),(60.5,86.5),(59,86.4),(58,87)]),
 ('Windward coastal shoulder',115,[(92.5,41.2),(94.2,41),(95.4,42),(94.6,43.3),(92.8,43.2),(92.2,42.5)]),
 ('Freyland landing shoulder',150,[(89.8,79),(91.1,79.1),(92.5,79.8),(92.7,81),(91.6,81.4),(90.2,80.6)]),
]
